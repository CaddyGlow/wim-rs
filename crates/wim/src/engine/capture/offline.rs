//! Native volume manifests enter the existing deferred WIM capture graph.

use super::{CaptureBinding, CaptureIdentity, CapturePlan, CapturedSource, CapturedStream};
use crate::engine::collections::FallibleSet as _;
use crate::{VolumeCaptureAudit, VolumeCaptureOptions};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use wim_format::{
    ParseError,
    metadata_write::{OwnedDentry, OwnedStream},
};

fn utf16(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

fn name_valid(name: &[u16], allow_empty: bool) -> bool {
    (allow_empty || !name.is_empty())
        && name.len() <= 255
        && !name.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
        && name != [46]
        && name != [46, 46]
}

fn security_valid(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || bytes[0] != 1 {
        return false;
    }
    let control = u16::from_le_bytes([bytes[2], bytes[3]]);
    if control & 0x8000 == 0 {
        return false;
    }
    for (field, acl, present) in [
        (4, false, true),
        (8, false, true),
        (12, true, control & 0x10 != 0),
        (16, true, control & 4 != 0),
    ] {
        let start =
            u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap_or([0; 4])) as usize;
        if start == 0 {
            continue;
        }
        if !present || start < 20 || !start.is_multiple_of(4) {
            return false;
        }
        let Some(header) = bytes.get(start..start.saturating_add(8)) else {
            return false;
        };
        if acl {
            if !matches!(header[0], 2 | 4) {
                return false;
            }
            let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
            let Some(data) = bytes.get(start..start.saturating_add(length)) else {
                return false;
            };
            if length < 8 {
                return false;
            }
            let count = u16::from_le_bytes([header[4], header[5]]);
            let mut position = 8usize;
            for _ in 0..count {
                let Some(ace) = data.get(position..position.saturating_add(4)) else {
                    return false;
                };
                let size = usize::from(u16::from_le_bytes([ace[2], ace[3]]));
                if size < 4 || !size.is_multiple_of(4) {
                    return false;
                }
                position += size;
                if position > length {
                    return false;
                }
            }
        } else if header[0] != 1
            || header[1] > 15
            || bytes
                .get(start..start + 8 + usize::from(header[1]) * 4)
                .is_none()
        {
            return false;
        }
    }
    true
}

fn tag(output: &mut Vec<u8>, kind: u32, data: &[u8]) -> Result<(), ParseError> {
    let size = u32::try_from(data.len()).map_err(|_| ParseError::InvalidMetadataResource)?;
    output
        .try_reserve(8 + data.len() + 7)
        .map_err(|_| ParseError::Nomem)?;
    output.extend_from_slice(&kind.to_le_bytes());
    output.extend_from_slice(&size.to_le_bytes());
    output.extend_from_slice(data);
    output.resize((output.len() + 7) & !7, 0);
    Ok(())
}

fn reparse(raw: &[u8], options: &VolumeCaptureOptions) -> Result<(Vec<u8>, bool), ParseError> {
    if raw.len() < 8
        || raw.len() > 16384
        || usize::from(u16::from_le_bytes([raw[4], raw[5]])) + 8 != raw.len()
    {
        return Err(ParseError::InvalidReparseData);
    }
    // App execution aliases carry provider metadata rather than a filesystem
    // name-surrogate target. Preserve those bytes without link relocation.
    if u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) == 0x8000_001b {
        return Ok((raw.to_vec(), false));
    }
    let link = super::reparse::parse(raw).ok_or(ParseError::Unsupported)?;
    if link.substitute.is_empty() || link.print.len() > 16384 || link.flags & !1 != 0 {
        return Err(ParseError::InvalidReparseData);
    }
    if link.tag == super::reparse::SYMLINK && link.flags & 1 != 0 {
        return Ok((raw.to_vec(), false));
    }
    let target: Vec<u16> = link
        .substitute
        .chunks_exact(2)
        .map(|word| u16::from_le_bytes([word[0], word[1]]))
        .collect();
    for alias in &options.volume_aliases {
        let alias: Vec<u16> = alias.encode_utf16().collect();
        if target.len() > alias.len()
            && target[alias.len()] == 92
            && target[..alias.len()].iter().zip(&alias).all(|(a, b)| {
                wim_format::ntfs_upcase::uppercase(*a) == wim_format::ntfs_upcase::uppercase(*b)
            })
        {
            let drive = options.installation_system_drive.as_deref().unwrap_or("X:");
            let prefix = format!(r"\??\{drive}");
            let mut substitute = utf16(&prefix.encode_utf16().collect::<Vec<_>>());
            substitute.extend_from_slice(&utf16(&target[alias.len()..]));
            return Ok((
                super::reparse::make(&link, &substitute, &substitute[8..])?,
                options.installation_system_drive.is_none(),
            ));
        }
    }
    if options.preserve_external_links {
        Ok((raw.to_vec(), false))
    } else {
        Err(ParseError::Unsupported)
    }
}

fn alias_valid(alias: &str) -> bool {
    let Some(root) = alias.strip_prefix(r"\??\") else {
        return false;
    };
    if root.len() == 2 {
        return root.as_bytes()[0].is_ascii_alphabetic() && root.as_bytes()[1] == b':';
    }
    let Some(guid) = root
        .strip_prefix("Volume{")
        .and_then(|r| r.strip_suffix('}'))
    else {
        return false;
    };
    guid.len() == 36
        && guid.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn plan(
    manifest: disk_capture::Manifest,
    options: &VolumeCaptureOptions,
) -> Result<(CapturePlan, VolumeCaptureAudit), ParseError> {
    if options
        .volume_aliases
        .iter()
        .any(|alias| !alias_valid(alias))
    {
        return Err(ParseError::InvalidParam);
    }
    if let Some(drive) = &options.installation_system_drive
        && (drive != "C:"
            || options.preserve_external_links
            || !options
                .volume_aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(r"\??\C:")))
    {
        return Err(ParseError::InvalidParam);
    }
    if manifest.nodes.is_empty() {
        return Err(ParseError::InvalidMetadataResource);
    }
    let session = crate::engine::handles::new_identity();
    let mut plan = CapturePlan::default();
    let mut audit = VolumeCaptureAudit::default();
    let mut sibling_names = HashSet::new();
    let mut security = HashMap::<Vec<u8>, u32>::new();
    let mut streams = HashMap::<(u64, Vec<u16>), Arc<CapturedStream>>::new();
    let mut inodes = HashMap::<u64, usize>::new();
    let mut inode_streams = HashMap::<u64, Vec<(Vec<u16>, Arc<dyn virtdisk::ReadAt>)>>::new();
    for (index, source) in manifest.nodes.into_iter().enumerate() {
        if source.attributes & 0x4000 != 0 {
            return Err(ParseError::Unsupported);
        }
        let folded: Vec<u16> = source
            .name
            .iter()
            .copied()
            .map(wim_format::ntfs_upcase::uppercase)
            .collect();
        if !sibling_names.insert((source.parent, folded)) {
            return Err(ParseError::InvalidMetadataResource);
        }
        if !name_valid(&source.name, index == 0) || !name_valid(&source.short_name, true) {
            return Err(ParseError::InvalidMetadataResource);
        }
        let mut names = HashSet::new();
        if source.streams.len() > u16::MAX as usize
            || source.streams.iter().any(|s| {
                !name_valid(&s.name, true)
                    || !names.insert(
                        s.name
                            .iter()
                            .copied()
                            .map(wim_format::ntfs_upcase::uppercase)
                            .collect::<Vec<_>>(),
                    )
            })
        {
            return Err(ParseError::InvalidMetadataResource);
        }
        if let Some(prior) = inode_streams.get(&source.file_id) {
            if prior.len() != source.streams.len()
                || !source.streams.iter().all(|stream| {
                    prior
                        .iter()
                        .any(|(name, data)| *name == stream.name && Arc::ptr_eq(data, &stream.data))
                })
            {
                return Err(ParseError::InvalidMetadataResource);
            }
        } else {
            inode_streams.insert(
                source.file_id,
                source
                    .streams
                    .iter()
                    .map(|s| (s.name.clone(), s.data.clone()))
                    .collect(),
            );
        }
        if index == 0 {
            if source.parent.is_some() || !source.name.is_empty() || source.attributes & 0x10 == 0 {
                return Err(ParseError::InvalidMetadataResource);
            }
        } else {
            let parent = source.parent.ok_or(ParseError::InvalidMetadataResource)?;
            if parent >= index || plan.tree.nodes[parent].attributes & (0x10 | 0x400) != 0x10 {
                return Err(ParseError::InvalidMetadataResource);
            }
            plan.tree.nodes[parent].children.push(index);
        }
        let identity = CaptureIdentity {
            session,
            device: manifest.volume_serial,
            inode: source.file_id,
        };
        let mut node = OwnedDentry::new(utf16(&source.name), source.attributes);
        node.short_name = utf16(&source.short_name);
        node.creation_time = source.creation_time;
        node.last_access_time = source.access_time;
        node.last_write_time = source.write_time;
        if !security_valid(&source.security_descriptor) {
            return Err(ParseError::InvalidMetadataResource);
        }
        node.security_id = if let Some(id) = security.get(&source.security_descriptor) {
            *id
        } else {
            let id = u32::try_from(plan.tree.security_descriptors.len())
                .map_err(|_| ParseError::InvalidMetadataResource)?;
            security.insert(source.security_descriptor.clone(), id);
            plan.tree
                .security_descriptors
                .push(source.security_descriptor);
            id
        };
        if !source.extended_attributes.is_empty() {
            let (packed, omitted) =
                crate::engine::ntfs_metadata::pack_portable_eas(&source.extended_attributes)?;
            audit.omitted_kernel_eas += omitted;
            audit.nodes_with_omitted_kernel_eas += usize::from(omitted != 0);
            if !packed.is_empty() {
                tag(&mut node.tagged_items, 2, &packed)?;
            }
        }
        if !source.object_id.is_empty() {
            if !matches!(source.object_id.len(), 16 | 64) {
                return Err(ParseError::InvalidMetadataResource);
            }
            tag(&mut node.tagged_items, 1, &source.object_id)?;
        }
        if let Some(class) = source.storage_class {
            if !matches!(class, 1 | 2) {
                return Err(ParseError::Unsupported);
            }
            tag(&mut node.tagged_items, 4, &class.to_le_bytes())?;
            audit.storage_class_nodes += 1;
        }
        if node.attributes & 0x200 != 0
            && let Some(stream) = source.streams.iter().find(|stream| stream.name.is_empty())
        {
            let holes = stream.data.sparse_holes().map_err(|_| ParseError::Read)?;
            let mut ranges = Vec::new();
            let mut previous_end = 0;
            for (start, end) in holes {
                if start < previous_end || start >= end || end > stream.data.len() {
                    return Err(ParseError::InvalidMetadataResource);
                }
                ranges.try_reserve(16).map_err(|_| ParseError::Nomem)?;
                ranges.extend_from_slice(&start.to_le_bytes());
                ranges.extend_from_slice(&end.to_le_bytes());
                previous_end = end;
            }
            if !ranges.is_empty() {
                audit.sparse_files += 1;
                audit.sparse_hole_ranges += ranges.len() / 16;
                tag(&mut node.tagged_items, 3, &ranges)?;
            }
        }
        if node.attributes & 0x400 != 0 {
            let (raw, fixed) = reparse(&source.reparse_data, options)?;
            node.inode_union = u64::from(u32::from_le_bytes(
                raw[..4]
                    .try_into()
                    .map_err(|_| ParseError::InvalidReparseData)?,
            )) | (u64::from(u16::from_le_bytes([raw[6], raw[7]])) << 32)
                | (u64::from(!fixed) << 48);
            let data = raw[8..].to_vec();
            plan.bindings.push(CaptureBinding {
                node: index,
                slot: 0,
                stream: Arc::new(CapturedStream {
                    size: data.len() as u64,
                    identity,
                    source: CapturedSource::Inline(data),
                }),
            });
        } else if !source.reparse_data.is_empty() {
            return Err(ParseError::InvalidReparseData);
        }
        for stream in source.streams {
            let slot = if stream.name.is_empty() && node.attributes & 0x400 == 0 {
                0
            } else {
                node.extra_streams.push(OwnedStream {
                    name: utf16(&stream.name),
                    ..Default::default()
                });
                node.extra_streams.len()
            };
            if stream.data.is_empty() {
                continue;
            }
            let size = stream.data.len();
            let shared = streams
                .entry((source.file_id, stream.name))
                .or_insert_with(|| {
                    Arc::new(CapturedStream {
                        size,
                        identity,
                        source: CapturedSource::Volume(stream.data),
                    })
                });
            if shared.size != size {
                return Err(ParseError::InvalidMetadataResource);
            }
            plan.bindings.push(CaptureBinding {
                node: index,
                slot,
                stream: shared.clone(),
            });
        }
        if node.attributes & (0x10 | 0x400) == 0 {
            if let Some(first) = inodes.get(&source.file_id).copied() {
                let prior = &plan.tree.nodes[first];
                if prior.attributes != node.attributes
                    || prior.security_id != node.security_id
                    || prior.creation_time != node.creation_time
                    || prior.last_access_time != node.last_access_time
                    || prior.last_write_time != node.last_write_time
                    || prior.tagged_items != node.tagged_items
                    || prior.extra_streams != node.extra_streams
                {
                    return Err(ParseError::InvalidMetadataResource);
                }
                let group = first as u64 + 1;
                plan.tree.nodes[first].inode_union = group;
                node.inode_union = group;
            } else {
                inodes.insert(source.file_id, index);
            }
        } else if inodes.insert(source.file_id, index).is_some() {
            // WIM's packed reparse union has no ordinary hard-link identifier.
            return Err(ParseError::Unsupported);
        }
        plan.tree.nodes.push(node);
        plan.identities.push(Some(identity));
    }
    plan.metadata_order()?;
    plan.tree.encode()?;
    Ok((plan, audit))
}

pub(crate) fn attach(
    handle: &mut crate::engine::WimHandle,
    image: usize,
    plan: CapturePlan,
) -> Result<(), ParseError> {
    let output = plan.tree.encode()?;
    let index = crate::engine::lookup::prepare_capture_index(handle, image, Some(&plan))?;
    let identity = crate::engine::handles::image_identity(handle, &handle.images[image]);
    let pending =
        crate::engine::handles::new_pending_metadata(crate::engine::handles::PendingMetadata {
            capture: Some(plan),
            metadata: output,
            hash: [0; 20],
        })?;
    handle
        .dirty_images
        .try_reserve(1)
        .map_err(|_| ParseError::Nomem)?;
    handle
        .dirty_images
        .try_insert(identity)
        .map_err(|_| ParseError::Nomem)?;
    handle.images[image] = crate::engine::HandleImage::Owned(crate::engine::handles::OwnedImage {
        metadata: Vec::new(),
        hash: [0; 20],
        pending: Some(pending),
        descriptor: None,
        identity,
    });
    handle.blob_index = index;
    handle.header.flags |= 0x80;
    Ok(())
}
