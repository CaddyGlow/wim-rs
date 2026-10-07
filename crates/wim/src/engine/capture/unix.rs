//! Unix capture scanner with explicit, unhashed stream ownership.
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use wim_format::{ParseError, metadata_write::OwnedDentry};

use super::{CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream};
use sha1::{Digest, Sha1};

use super::common::{CaptureConfig, ScanCallback, ScanEvent};

fn timestamp(seconds: i64, nanos: i64) -> u64 {
    ((i128::from(seconds) + 11_644_473_600) * 10_000_000 + i128::from(nanos) / 100)
        .clamp(0, i128::from(u64::MAX)) as u64
}
fn tag(node: &mut OwnedDentry, id: u32, bytes: &[u8]) -> Result<(), ParseError> {
    let size = u32::try_from(bytes.len()).map_err(|_| ParseError::Nomem)?;
    node.tagged_items
        .try_reserve(bytes.len() + 15)
        .map_err(|_| ParseError::Nomem)?;
    node.tagged_items.extend_from_slice(&id.to_le_bytes());
    node.tagged_items.extend_from_slice(&size.to_le_bytes());
    node.tagged_items.extend_from_slice(bytes);
    node.tagged_items
        .resize((node.tagged_items.len() + 7) & !7, 0);
    Ok(())
}
fn name(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    Ok(wim_format::platform_text::wtf8_to_utf16(bytes)
        .inspect_err(|_| {
            #[cfg(target_os = "linux")]
            // SAFETY: errno is calling-thread storage.
            unsafe {
                *libc::__errno_location() = libc::EILSEQ;
            }
        })?
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect())
}
fn symlink_data(target: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut units = wim_format::platform_text::wtf8_to_utf16(target)?;
    for unit in &mut units {
        *unit = match *unit {
            47 => 92,
            92 => 47,
            value => value,
        };
    }
    let absolute = units.first().is_none_or(|&u| u == 92);
    let mut substitute = if absolute {
        vec![92, 63, 63, 92, 67, 58]
    } else {
        Vec::new()
    };
    substitute.extend_from_slice(&units);
    let print = if absolute {
        &substitute[4..]
    } else {
        &substitute[..]
    };
    let sub_len = u16::try_from(substitute.len() * 2).map_err(|_| ParseError::Readlink)?;
    let print_len = u16::try_from(print.len() * 2).map_err(|_| ParseError::Readlink)?;
    let mut bytes = Vec::new();
    for value in [0, sub_len, sub_len + 2, print_len] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(u32::from(!absolute)).to_le_bytes());
    bytes.extend(substitute.iter().flat_map(|u| u.to_le_bytes()));
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend(print.iter().flat_map(|u| u.to_le_bytes()));
    bytes.extend_from_slice(&[0, 0]);
    if bytes.len() + 8 > 16_384 {
        return Err(ParseError::Readlink);
    }
    Ok(bytes)
}
fn relativize(target: &[u8], root: CaptureIdentity) -> &[u8] {
    let mut pos = 0;
    while pos < target.len() {
        while target.get(pos) == Some(&b'/') {
            pos += 1;
        }
        if pos == target.len() {
            break;
        }
        while pos < target.len() && target[pos] != b'/' {
            pos += 1;
        }
        let path = Path::new(std::ffi::OsStr::from_bytes(&target[..pos]));
        let Ok(stat) = std::fs::metadata(path) else {
            break;
        };
        if stat.dev() == root.device && stat.ino() == root.inode {
            return &target[pos..];
        }
    }
    target
}

#[cfg(target_os = "linux")]
fn xattrs(path: &Path, node: &mut OwnedDentry) -> Result<(), ParseError> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ParseError::InvalidParam)?;
    // SAFETY: The terminated path and caller-owned buffers remain live.
    let length = unsafe { libc::llistxattr(path.as_ptr(), std::ptr::null_mut(), 0) };
    if length < 0 {
        let code = std::io::Error::last_os_error().raw_os_error();
        return if matches!(code, Some(libc::ENOTSUP | libc::ENOSYS)) {
            Ok(())
        } else {
            Err(ParseError::Stat)
        };
    }
    if length == 0 {
        return Ok(());
    }
    let mut names = vec![0u8; length as usize];
    // SAFETY: names spans the size queried above; races produce an error.
    let length = unsafe { libc::llistxattr(path.as_ptr(), names.as_mut_ptr().cast(), names.len()) };
    if length < 0 {
        return Err(ParseError::Stat);
    }
    names.truncate(length as usize);
    let mut entries = Vec::new();
    for bytes in names.split(|&b| b == 0).filter(|b| !b.is_empty()) {
        let attr = CString::new(bytes).map_err(|_| ParseError::Stat)?;
        // SAFETY: Both strings are terminated and readable.
        let length =
            unsafe { libc::lgetxattr(path.as_ptr(), attr.as_ptr(), std::ptr::null_mut(), 0) };
        if length < 0 {
            return Err(ParseError::Stat);
        }
        if length > 65535 || bytes.len() > 255 {
            continue;
        }
        let mut value = vec![0u8; length as usize];
        // SAFETY: value spans the queried size; concurrent changes fail safely.
        let length = unsafe {
            libc::lgetxattr(
                path.as_ptr(),
                attr.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if length < 0 {
            return Err(ParseError::Stat);
        }
        value.truncate(length as usize);
        entries.extend_from_slice(&(length as u16).to_le_bytes());
        entries.extend_from_slice(&[bytes.len() as u8, 0]);
        entries.extend_from_slice(bytes);
        entries.push(0);
        entries.extend(value);
    }
    tag(node, 2, &entries)
}
#[cfg(not(target_os = "linux"))]
fn xattrs(_path: &Path, _node: &mut OwnedDentry) -> Result<(), ParseError> {
    Ok(())
}

struct PriorInode {
    node: OwnedDentry,
    streams: Vec<(usize, Arc<CapturedStream>)>,
}
struct Scanner<'a> {
    session: u64,
    prior: HashMap<(u64, u64), PriorInode>,
    source: &'a Path,
    flags: i32,
    config: &'a CaptureConfig,
    progress: &'a mut ScanCallback<'a>,
    plan: CapturePlan,
    inodes: HashMap<(u64, u64), usize>,
    canonical: HashSet<usize>,
    root_identity: Option<CaptureIdentity>,
    counts: (u64, u64, u64),
    last_path: Option<PathBuf>,
    last_status: i32,
}
impl Scanner<'_> {
    fn event(
        &mut self,
        message: i32,
        path: Option<&Path>,
        status: i32,
        target: Option<&[u8]>,
    ) -> Result<bool, ParseError> {
        let mut event = ScanEvent {
            message,
            source: self.source,
            current_path: path,
            status,
            symlink_target: target,
            directories: self.counts.0,
            nondirectories: self.counts.1,
            bytes: self.counts.2,
            exclude: false,
            error: None,
        };
        (self.progress)(&mut event)?;
        Ok(event.exclude)
    }
    fn report(
        &mut self,
        path: &Path,
        status: i32,
        index: Option<usize>,
        target: Option<&[u8]>,
    ) -> Result<(), ParseError> {
        if (status == 0 && self.flags & 4 == 0) || (status != 0 && self.flags & 0x80 == 0) {
            return Ok(());
        }
        if let Some(index) = index {
            if self.plan.tree.nodes[index].attributes & (0x10 | 0x400) == 0x10 {
                self.counts.0 += 1;
            } else {
                self.counts.1 += 1;
            }
            if self.canonical.contains(&index) {
                self.counts.2 += self
                    .plan
                    .bindings
                    .iter()
                    .filter(|b| b.node == index)
                    .map(|b| b.stream.size)
                    .sum::<u64>();
            }
        }
        self.last_path = Some(path.to_owned());
        self.last_status = status;
        self.event(10, Some(path), status, target).map(|_| ())
    }
    fn visit(
        &mut self,
        path: &Path,
        root: bool,
        image_root: bool,
    ) -> Result<Option<usize>, ParseError> {
        let before = self.plan.tree.nodes.len();
        let result = self.visit_inner(path, root, image_root);
        match result {
            Err(error)
                if !matches!(
                    error,
                    ParseError::AbortedByProgress | ParseError::UnknownProgressStatus
                ) =>
            {
                self.plan.tree.nodes.truncate(before);
                self.plan.identities.truncate(before);
                self.plan.bindings.retain(|binding| binding.node < before);
                self.inodes.retain(|_, node| *node < before);
                self.canonical.retain(|&node| node < before);
                let mut event = ScanEvent {
                    message: 31,
                    source: self.source,
                    current_path: Some(path),
                    status: self.last_status,
                    symlink_target: None,
                    directories: self.counts.0,
                    nondirectories: self.counts.1,
                    bytes: self.counts.2,
                    exclude: false,
                    error: Some(error),
                };
                (self.progress)(&mut event)?;
                if event.exclude { Ok(None) } else { Err(error) }
            }
            other => other,
        }
    }
    fn visit_inner(
        &mut self,
        path: &Path,
        root: bool,
        image_root: bool,
    ) -> Result<Option<usize>, ParseError> {
        if self.config.excluded(
            path.strip_prefix(self.source)
                .unwrap_or(path)
                .as_os_str()
                .as_bytes(),
        ) || (self.flags & 0x4000 != 0 && self.event(30, Some(path), 0, None)?)
        {
            self.report(path, 1, None, None)?;
            return Ok(None);
        }
        let metadata = if self.flags & 2 != 0 || (root && image_root) {
            std::fs::metadata(path)
        } else {
            std::fs::symlink_metadata(path)
        }
        .map_err(|error| {
            #[cfg(target_os = "linux")]
            // SAFETY: Preserve the actual failed filesystem operation's errno.
            unsafe {
                *libc::__errno_location() = error.raw_os_error().unwrap_or(libc::EIO);
            }
            ParseError::Stat
        })?;
        let mode = metadata.mode() & libc::S_IFMT;
        if self.flags & 0x10 == 0 && !matches!(mode, libc::S_IFREG | libc::S_IFDIR | libc::S_IFLNK)
        {
            if self.flags & 0x400 != 0 {
                return Err(ParseError::UnsupportedFile);
            }
            self.report(path, 2, None, None)?;
            return Ok(None);
        }
        let identity = CaptureIdentity {
            session: self.session,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        if root && image_root {
            self.root_identity = Some(identity);
        }
        let node_name = if root {
            Vec::new()
        } else {
            name(path.file_name().ok_or(ParseError::InvalidParam)?.as_bytes())?
        };
        let index = self.plan.tree.nodes.len();
        if mode != libc::S_IFDIR
            && let Some(&first) = self.inodes.get(&(identity.device, identity.inode))
        {
            let mut node = self.plan.tree.nodes[first].clone();
            node.name = node_name;
            if node.attributes & 0x400 == 0 {
                let id = first as u64 + 1;
                node.inode_union = id;
                self.plan.tree.nodes[first].inode_union = id;
            }
            self.plan.tree.nodes.push(node);
            self.plan.identities.push(Some(identity));
            let aliases: Vec<_> = self
                .plan
                .bindings
                .iter()
                .filter(|b| b.node == first)
                .map(|b| CaptureBinding {
                    node: index,
                    slot: b.slot,
                    stream: b.stream.clone(),
                })
                .collect();
            self.plan.bindings.extend(aliases);
            self.report(path, 0, Some(index), None)?;
            return Ok(Some(index));
        }
        if mode != libc::S_IFDIR
            && let Some(previous) = self.prior.get(&(identity.device, identity.inode))
        {
            let mut node = previous.node.clone();
            node.name = node_name;
            self.plan.tree.nodes.push(node);
            self.plan.identities.push(Some(identity));
            self.plan
                .bindings
                .extend(
                    previous
                        .streams
                        .iter()
                        .map(|(slot, stream)| CaptureBinding {
                            node: index,
                            slot: *slot,
                            stream: stream.clone(),
                        }),
                );
            self.inodes.insert((identity.device, identity.inode), index);
            self.report(path, 0, Some(index), None)?;
            return Ok(Some(index));
        }
        let mut node = OwnedDentry::new(
            node_name,
            match mode {
                libc::S_IFDIR => 0x10,
                libc::S_IFREG if metadata.blocks() < metadata.size().div_ceil(512) => 0x200,
                libc::S_IFREG => 0x80,
                _ => 0,
            },
        );
        node.creation_time = timestamp(metadata.mtime(), metadata.mtime_nsec());
        node.last_write_time = node.creation_time;
        node.last_access_time = timestamp(metadata.atime(), metadata.atime_nsec());
        if self.flags & 0x10 != 0 {
            let data: Vec<_> = [
                metadata.uid(),
                metadata.gid(),
                metadata.mode(),
                metadata.rdev() as u32,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            tag(&mut node, 0x337dd873, &data)?;
            xattrs(path, &mut node)?;
        }
        self.plan.tree.nodes.push(node);
        self.plan.identities.push(Some(identity));
        self.inodes.insert((identity.device, identity.inode), index);
        self.canonical.insert(index);
        if mode == libc::S_IFREG && metadata.size() != 0 {
            self.plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size: metadata.size(),
                    identity,
                    source: CapturedSource::File(path.to_owned()),
                }),
            });
        } else if mode == libc::S_IFDIR {
            let directory = std::fs::read_dir(path).map_err(|_| ParseError::Opendir)?;
            for entry in directory {
                let entry = entry.map_err(|_| ParseError::Read)?;
                if let Some(child) = self.visit(&entry.path(), false, image_root)? {
                    self.plan.tree.nodes[index].children.push(child);
                }
            }
        } else if mode == libc::S_IFLNK {
            let target = std::fs::read_link(path).map_err(|_| ParseError::Readlink)?;
            let original = target.as_os_str().as_bytes();
            let mut rewritten = original;
            let mut rp_flags = 1u64;
            if original.starts_with(b"/") && self.flags & 0x100 != 0 {
                if let Some(root) = self.root_identity {
                    rewritten = relativize(original, root);
                }
                let fixed = rewritten.as_ptr() != original.as_ptr();
                if fixed {
                    rp_flags = 0;
                }
                self.report(path, if fixed { 3 } else { 4 }, None, Some(original))?;
            }
            let data = symlink_data(rewritten)?;
            let node = &mut self.plan.tree.nodes[index];
            node.main_hash = Sha1::digest(&data).into();
            node.attributes = 0x400
                | if std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
                    0x10
                } else {
                    0
                };
            node.inode_union = 0xa000000c | (rp_flags << 48);
            self.plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size: data.len() as u64,
                    identity,
                    source: CapturedSource::Inline(data),
                }),
            });
        }
        self.report(path, 0, Some(index), None)?;
        Ok(Some(index))
    }
}
/// Scan an already validated source command. File content remains deferred.
/// Flags must include facade-normalized verbosity and reparse fixup defaults.
pub fn scan_source<'a>(
    source: &'a Path,
    image_root: bool,
    flags: i32,
    config: &'a CaptureConfig,
    callback: &'a mut ScanCallback<'a>,
) -> Result<CapturePlan, ParseError> {
    scan_source_seeded(
        source,
        image_root,
        flags,
        config,
        &CapturePlan::default(),
        crate::engine::handles::new_identity(),
        callback,
    )
}
/// Scan another command in the same update transaction, reusing inode ownership
/// and unique-byte counts from its earlier scanned branches.
pub fn scan_source_seeded<'a>(
    source: &'a Path,
    image_root: bool,
    flags: i32,
    config: &'a CaptureConfig,
    existing: &CapturePlan,
    session: u64,
    callback: &'a mut ScanCallback<'a>,
) -> Result<CapturePlan, ParseError> {
    let mut prior = HashMap::new();
    for (index, identity) in existing.identities.iter().enumerate() {
        if let Some(identity) = identity
            && identity.session == session
        {
            let node = existing
                .tree
                .nodes
                .get(index)
                .ok_or(ParseError::InvalidMetadataResource)?;
            if node.attributes & (0x10 | 0x400) == 0x10 {
                continue;
            }
            let streams = existing
                .bindings
                .iter()
                .filter(|b| b.node == index)
                .map(|b| (b.slot, b.stream.clone()))
                .collect();
            prior
                .entry((identity.device, identity.inode))
                .or_insert(PriorInode {
                    node: node.clone(),
                    streams,
                });
        }
    }
    let mut scanner = Scanner {
        session,
        prior,
        source,
        flags,
        config,
        progress: callback,
        plan: CapturePlan::default(),
        inodes: HashMap::new(),
        canonical: HashSet::new(),
        root_identity: None,
        counts: (0, 0, 0),
        last_path: None,
        last_status: 0,
    };
    scanner.event(9, None, 0, None)?;
    scanner.visit(source, true, image_root)?;
    let last_path = scanner.last_path.clone();
    scanner.event(11, last_path.as_deref(), scanner.last_status, None)?;
    if image_root
        && scanner
            .plan
            .tree
            .nodes
            .first()
            .is_some_and(|n| n.attributes & 0x10 == 0)
    {
        return Err(ParseError::Notdir);
    }
    Ok(scanner.plan)
}
