#![cfg(feature = "disk-capture")]
//! Regression gates for translating retained offline NTFS streams into WIM.
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
use wim::{Compression, OpenOptions, VolumeCaptureOptions, Wim};
use wim_format::{
    archive::Archive,
    metadata::{Metadata, StreamType},
};
use windows_disk::ReadAt;
use windows_ntfs::{Manifest, Node, Stream};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "offline-wim-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Data {
    bytes: Vec<u8>,
    reads: Arc<AtomicUsize>,
    fail: bool,
}
impl ReadAt for Data {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> io::Result<()> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source read failure",
            ));
        }
        let offset = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let end = offset
            .checked_add(destination.len())
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        destination.copy_from_slice(
            self.bytes
                .get(offset..end)
                .ok_or(io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
}
fn data(bytes: &[u8], reads: &Arc<AtomicUsize>) -> Arc<dyn ReadAt> {
    Arc::new(Data {
        bytes: bytes.to_vec(),
        reads: reads.clone(),
        fail: false,
    })
}
fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}
fn raw_units(text: &[u16]) -> Vec<u8> {
    text.iter().flat_map(|u| u.to_le_bytes()).collect()
}
fn descriptor() -> Vec<u8> {
    let mut sd = vec![0; 84];
    sd[0] = 1;
    sd[2..4].copy_from_slice(&0x8014u16.to_le_bytes()); // self-relative, DACL/SACL present
    for (field, offset) in [(4, 20u32), (8, 32), (16, 48), (12, 56)] {
        sd[field..field + 4].copy_from_slice(&offset.to_le_bytes());
    }
    // Owner S-1-5-18; group S-1-5-32-544.
    sd[20..32].copy_from_slice(&[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]);
    sd[32..48].copy_from_slice(&[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0]);
    sd[48] = 2;
    sd[50..52].copy_from_slice(&8u16.to_le_bytes());
    sd[56] = 2;
    sd[58..60].copy_from_slice(&28u16.to_le_bytes());
    sd[60..62].copy_from_slice(&1u16.to_le_bytes());
    // Mandatory-label SACL: no-write-up, high-integrity S-1-16-12288.
    sd[64..84].copy_from_slice(&[
        17, 0, 20, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 16, 0, 48, 0, 0,
    ]);
    sd
}
fn node(parent: Option<usize>, name: &str, file_id: u64, attributes: u32) -> Node {
    Node {
        storage_class: None,
        parent,
        name: units(name),
        short_name: Vec::new(),
        file_id,
        attributes,
        creation_time: 132_000_000_000_000_001,
        access_time: 132_000_000_000_000_002,
        write_time: 132_000_000_000_000_003,
        security_descriptor: descriptor(),
        reparse_data: Vec::new(),
        extended_attributes: Vec::new(),
        object_id: Vec::new(),
        streams: Vec::new(),
    }
}
fn manifest(nodes: Vec<Node>) -> Manifest {
    Manifest {
        nodes,
        volume_serial: 0x1234,
    }
}
fn stream(name: &[u16], source: Arc<dyn ReadAt>) -> Stream {
    Stream {
        name: name.to_vec(),
        data: source,
    }
}
fn root() -> Node {
    node(None, "", 5, 0x10)
}
fn relative_link(target: &str) -> Vec<u8> {
    let name = raw_units(&units(target));
    let mut raw = Vec::new();
    raw.extend_from_slice(&0xa000000cu32.to_le_bytes());
    raw.extend_from_slice(&((12 + name.len()) as u16).to_le_bytes());
    raw.extend_from_slice(&[0; 2]);
    raw.extend_from_slice(&0u16.to_le_bytes());
    raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
    raw.extend_from_slice(&0u16.to_le_bytes());
    raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
    raw.extend_from_slice(&1u32.to_le_bytes());
    raw.extend_from_slice(&name);
    raw
}
fn entry_index(metadata: &Metadata<'_>, name: &[u16]) -> usize {
    metadata
        .nodes
        .iter()
        .position(|n| n.entry.name == raw_units(name))
        .unwrap()
}

#[test]
fn deferred_streams_preserve_hardlinks_ads_security_and_tagged_metadata() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let main = data(b"main deferred file data", &reads);
    let named = data(b"named alternate stream", &reads);
    let mut first = node(Some(0), "long filename.txt", 33, 0x23);
    first.short_name = units("LONGFI~1.TXT");
    first.object_id = (0..64u8).collect();
    first.extended_attributes = vec![0, 0, 0, 0, 0x80, 1, 3, 0, b'A', 0, b'x', b'y', b'z'];
    first.streams = vec![
        stream(&[], main.clone()),
        stream(&[0xd800, 0x41], named.clone()),
        stream(&units("empty"), data(b"", &reads)),
    ];
    let mut alias = node(Some(0), "alias.txt", 33, 0x23);
    alias.object_id = first.object_id.clone();
    alias.extended_attributes = first.extended_attributes.clone();
    alias.streams = first.streams.clone();
    let mut directory = node(Some(0), "directory", 34, 0x10);
    directory
        .streams
        .push(stream(&units("dirads"), data(b"directory-owned", &reads)));
    let mut root = root();
    root.streams
        .push(stream(&units("rootads"), data(b"root-owned", &reads)));
    let mut image = Wim::new(Compression::Lzx).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root, first, alias, directory]),
            "offline",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "capture must defer stream I/O"
    );
    drop(main);
    drop(named); // the capture graph must retain every deferred reader

    let output = fixture.0.join("output.wim");
    image.write(&output).unwrap();
    assert!(reads.load(Ordering::Relaxed) > 0);
    let mut reopened = Wim::open(
        &output,
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
    )
    .unwrap();
    reopened.verify().unwrap();
    let bytes = std::fs::read(&output).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    let raw = archive.read_metadata(1).unwrap();
    let metadata = Metadata::parse(&raw).unwrap();
    assert_eq!(metadata.nodes.len(), 4);
    assert_eq!(metadata.security.descriptors.len(), 1);
    let first_index = entry_index(&metadata, &units("long filename.txt"));
    let alias_index = entry_index(&metadata, &units("alias.txt"));
    assert_eq!(
        metadata.nodes[first_index].inode,
        metadata.nodes[alias_index].inode
    );
    let entry = metadata.inode_entry(first_index).unwrap();
    assert_eq!(entry.attributes, 0x23);
    assert_eq!(entry.creation_time, 132_000_000_000_000_001);
    assert_eq!(entry.last_access_time, 132_000_000_000_000_002);
    assert_eq!(entry.last_write_time, 132_000_000_000_000_003);
    assert_eq!(
        metadata.nodes[first_index].entry.short_name,
        raw_units(&units("LONGFI~1.TXT"))
    );
    assert_eq!(
        metadata.security_descriptor(first_index),
        Some(descriptor().as_slice())
    );
    assert_eq!(
        entry.tagged_item(1, 64).unwrap()[..64],
        (0..64u8).collect::<Vec<_>>()
    );
    assert_eq!(
        entry.tagged_item(2, 9).unwrap()[..9],
        [3, 0, 1, 0x80, b'A', 0, b'x', b'y', b'z']
    );
    for (node, stream_name, expected) in [
        (
            first_index,
            Vec::new(),
            b"main deferred file data".as_slice(),
        ),
        (
            first_index,
            vec![0xd800, 0x41],
            b"named alternate stream".as_slice(),
        ),
        (0, units("rootads"), b"root-owned".as_slice()),
        (
            entry_index(&metadata, &units("directory")),
            units("dirads"),
            b"directory-owned".as_slice(),
        ),
    ] {
        let entry = metadata.inode_entry(node).unwrap();
        let blob = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::Data && s.name == raw_units(&stream_name))
            .unwrap();
        assert_eq!(archive.read_blob(&blob.hash).unwrap(), expected);
    }
    let empty = entry
        .streams
        .iter()
        .find(|s| s.name == raw_units(&units("empty")))
        .unwrap();
    assert_eq!(empty.hash, [0; 20]);
    assert_eq!(
        archive
            .lookup
            .blobs
            .iter()
            .find(|b| archive.read_blob(&b.hash).unwrap() == b"main deferred file data")
            .unwrap()
            .reference_count,
        2
    );
}

#[test]
fn reparse_owned_unnamed_and_named_data_survive_repeated_writes() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let raw_reparse = relative_link("target.txt");
    let mut link = node(Some(0), "link", 36, 0x420);
    link.reparse_data = raw_reparse.clone();
    link.streams = vec![
        stream(&units("ads"), data(b"link named data", &reads)),
        stream(&[], data(b"link unnamed data", &reads)),
    ];
    let mut image = Wim::new(Compression::None).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), link]),
            "links",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    for iteration in 0..2 {
        let output = fixture.0.join(format!("links-{iteration}.wim"));
        image.write(&output).unwrap();
        let bytes = std::fs::read(output).unwrap();
        let archive = Archive::open(&bytes).unwrap();
        let raw = archive.read_metadata(1).unwrap();
        let metadata = Metadata::parse(&raw).unwrap();
        let entry = &metadata.nodes[entry_index(&metadata, &units("link"))].entry;
        let offset = entry.offset;
        assert_eq!(&raw[offset + 64..offset + 84], &[0; 20]);
        assert_eq!(u16::from_le_bytes([raw[offset + 96], raw[offset + 97]]), 3);
        assert_eq!(
            entry
                .streams
                .iter()
                .filter(|s| s.kind == StreamType::ReparsePoint)
                .count(),
            1
        );
        let rp = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::ReparsePoint)
            .unwrap();
        assert_eq!(archive.read_blob(&rp.hash).unwrap(), raw_reparse[8..]);
        // Inspect serialized references independently of the tolerant stream classifier.
        let mut slot =
            offset + u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap()) as usize;
        assert_eq!(&raw[slot + 16..slot + 36], &rp.hash);
        assert_eq!(u16::from_le_bytes([raw[slot + 36], raw[slot + 37]]), 0);
        slot += u64::from_le_bytes(raw[slot..slot + 8].try_into().unwrap()) as usize;
        assert_eq!(u16::from_le_bytes([raw[slot + 36], raw[slot + 37]]), 0);
        let unnamed = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::Data && s.name.is_empty())
            .unwrap();
        assert_eq!(&raw[slot + 16..slot + 36], &unnamed.hash);
        slot += u64::from_le_bytes(raw[slot..slot + 8].try_into().unwrap()) as usize;
        assert_eq!(&raw[slot + 38..slot + 44], &raw_units(&units("ads")));
        for (name, expected) in [
            (Vec::new(), b"link unnamed data".as_slice()),
            (units("ads"), b"link named data".as_slice()),
        ] {
            let data = entry
                .streams
                .iter()
                .find(|s| s.kind == StreamType::Data && s.name == raw_units(&name))
                .unwrap();
            assert_eq!(archive.read_blob(&data.hash).unwrap(), expected);
        }
        let mut reopened = Wim::open(
            &fixture.0.join(format!("links-{iteration}.wim")),
            OpenOptions::default(),
        )
        .unwrap();
        reopened.verify().unwrap();
    }
}

#[test]
fn deferred_reparse_empty_unnamed_data_and_ads_use_dism_extra_stream_layout() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let raw_reparse = relative_link("target.txt");
    let mut link = node(Some(0), "relative.link", 36, 0x420);
    link.reparse_data = raw_reparse.clone();
    link.streams = vec![
        stream(&[], data(b"", &reads)),
        stream(&units("ads"), data(b"named", &reads)),
    ];
    let mut image = Wim::new(Compression::None).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), link]),
            "links",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    for iteration in 0..2 {
        let output = fixture.0.join(format!("empty-link-{iteration}.wim"));
        image.write(&output).unwrap();
        let bytes = std::fs::read(output).unwrap();
        let archive = Archive::open(&bytes).unwrap();
        let raw = archive.read_metadata(1).unwrap();
        let metadata = Metadata::parse(&raw).unwrap();
        let entry = &metadata.nodes[entry_index(&metadata, &units("relative.link"))].entry;
        let offset = entry.offset;
        assert_eq!(&raw[offset + 64..offset + 84], &[0; 20]);
        assert_eq!(u16::from_le_bytes([raw[offset + 96], raw[offset + 97]]), 3);
        let rp = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::ReparsePoint)
            .unwrap();
        assert_eq!(archive.read_blob(&rp.hash).unwrap(), raw_reparse[8..]);
        let unnamed = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::Data && s.name.is_empty())
            .unwrap();
        assert_eq!(unnamed.hash, [0; 20]);
        let named = entry
            .streams
            .iter()
            .find(|s| s.name == raw_units(&units("ads")))
            .unwrap();
        assert_eq!(archive.read_blob(&named.hash).unwrap(), b"named");
    }
}

#[test]
fn malformed_manifest_is_rejected_without_adding_an_image() {
    let mut invalid = vec![
        manifest(Vec::new()),
        manifest(vec![node(None, "namedroot", 5, 0x10)]),
        manifest(vec![node(None, "", 5, 0x20)]),
        manifest(vec![root(), node(Some(1), "child", 22, 0x20)]),
    ];
    let mut sd = root();
    sd.security_descriptor.clear();
    invalid.push(manifest(vec![sd]));
    let mut object = root();
    object.object_id = vec![0; 17];
    invalid.push(manifest(vec![object]));
    let mut rp = node(Some(0), "bad-rp", 23, 0x420);
    rp.reparse_data = vec![0; 8];
    invalid.push(manifest(vec![root(), rp]));
    let mut image = Wim::new(Compression::None).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root()]),
            "existing",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    for manifest in invalid {
        assert!(
            image
                .capture_ntfs(manifest, "bad", &VolumeCaptureOptions::default())
                .is_err()
        );
        assert_eq!(image.info().unwrap().image_count, 1);
    }
}

#[test]
fn app_execution_alias_preserves_opaque_provider_bytes() {
    let fixture = Fixture::new();
    let mut alias = node(Some(0), "app.exe", 99, 0x420);
    let mut payload = 3u32.to_le_bytes().to_vec();
    payload.extend_from_slice(&raw_units(&units(
        "Package\0Entry\0C:\\Program Files\\App.exe\0",
    )));
    alias
        .reparse_data
        .extend_from_slice(&0x8000_001bu32.to_le_bytes());
    alias
        .reparse_data
        .extend_from_slice(&(payload.len() as u16).to_le_bytes());
    alias.reparse_data.extend_from_slice(&0u16.to_le_bytes());
    alias.reparse_data.extend_from_slice(&payload);
    let mut image = Wim::new(Compression::Lzx).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), alias]),
            "opaque",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    let output = fixture.0.join("opaque.wim");
    image.write(&output).unwrap();
    let bytes = std::fs::read(output).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    let raw_metadata = archive.read_metadata(1).unwrap();
    let metadata = Metadata::parse(&raw_metadata).unwrap();
    let index = entry_index(&metadata, &units("app.exe"));
    let entry = metadata.inode_entry(index).unwrap();
    let stream = entry
        .streams
        .iter()
        .find(|s| s.kind == StreamType::ReparsePoint)
        .unwrap();
    assert_eq!(archive.read_blob(&stream.hash).unwrap(), payload);
}

#[test]
fn source_read_failure_is_not_replaced_with_zero_data() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let mut file = node(Some(0), "failed", 55, 0x20);
    file.streams.push(stream(
        &[],
        Arc::new(Data {
            bytes: vec![0x77; 1024],
            reads,
            fail: true,
        }),
    ));
    let mut image = Wim::new(Compression::None).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), file]),
            "failing",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    assert!(image.write(&fixture.0.join("failed.wim")).is_err());
}

#[test]
fn directory_junction_ads_serialize_rp_first_without_directory_data() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let mut junction = node(Some(0), "directory.link", 92, 0x410);
    junction.reparse_data = relative_link("\\??\\C:\\Directory");
    junction.reparse_data[..4].copy_from_slice(&0xa000_0003u32.to_le_bytes());
    junction.reparse_data.drain(16..20);
    let length = (junction.reparse_data.len() - 8) as u16;
    junction.reparse_data[4..6].copy_from_slice(&length.to_le_bytes());
    junction.streams = vec![stream(&units("owned-named"), data(b"junction-ads", &reads))];
    let mut image = Wim::new(Compression::None).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), junction]),
            "junction",
            &VolumeCaptureOptions {
                volume_aliases: vec!["\\??\\C:".into()],
                ..VolumeCaptureOptions::default()
            },
        )
        .unwrap();
    for iteration in 0..2 {
        let output = fixture.0.join(format!("junction-{iteration}.wim"));
        image.write(&output).unwrap();
        let bytes = std::fs::read(output).unwrap();
        let archive = Archive::open(&bytes).unwrap();
        let raw = archive.read_metadata(1).unwrap();
        let metadata = Metadata::parse(&raw).unwrap();
        let entry = &metadata.nodes[entry_index(&metadata, &units("directory.link"))].entry;
        let offset = entry.offset;
        assert_eq!(&raw[offset + 64..offset + 84], &[0; 20]);
        assert_eq!(u16::from_le_bytes([raw[offset + 96], raw[offset + 97]]), 2);
        let rp = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::ReparsePoint)
            .unwrap();
        assert_ne!(rp.hash, [0; 20]);
        assert!(
            !entry
                .streams
                .iter()
                .any(|s| s.kind == StreamType::Data && s.name.is_empty())
        );
        let named = entry
            .streams
            .iter()
            .find(|s| s.name == raw_units(&units("owned-named")))
            .unwrap();
        assert_eq!(archive.read_blob(&named.hash).unwrap(), b"junction-ads");
        let extra =
            offset + u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap()) as usize;
        assert_eq!(&raw[extra + 16..extra + 36], &rp.hash);
        assert_eq!(&raw[extra + 36..extra + 38], &[0, 0]);
    }
}

#[test]
#[ignore = "requires independent upstream wimlib-imagex in PATH"]
fn independently_verifies_offline_archive_with_owned_link_streams() {
    let fixture = Fixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let mut regular = node(Some(0), "target.txt", 89, 0x20);
    regular.streams = vec![stream(&[], data(b"regular", &reads))];
    let mut link = node(Some(0), "link", 90, 0x420);
    link.reparse_data = relative_link("target.txt");
    link.streams = vec![
        stream(&[], data(b"owned-default", &reads)),
        stream(&units("owned-named"), data(b"owned-ads", &reads)),
    ];
    let mut app = node(Some(0), "application.exe", 91, 0x420);
    let mut opaque = 3u32.to_le_bytes().to_vec();
    opaque.extend_from_slice(&raw_units(&units("Package\0Entry\0C:\\App.exe\0")));
    app.reparse_data
        .extend_from_slice(&0x8000_001bu32.to_le_bytes());
    app.reparse_data
        .extend_from_slice(&(opaque.len() as u16).to_le_bytes());
    app.reparse_data.extend_from_slice(&0u16.to_le_bytes());
    app.reparse_data.extend_from_slice(&opaque);
    let mut junction = node(Some(0), "directory.link", 92, 0x410);
    junction.reparse_data = relative_link("\\??\\C:\\Directory");
    junction.reparse_data[..4].copy_from_slice(&0xa000_0003u32.to_le_bytes());
    junction.reparse_data.drain(16..20);
    let length = (junction.reparse_data.len() - 8) as u16;
    junction.reparse_data[4..6].copy_from_slice(&length.to_le_bytes());
    junction.streams = vec![stream(
        &units("junction-owned"),
        data(b"junction-ads", &reads),
    )];
    let mut image = Wim::new(Compression::Lzx).unwrap();
    image
        .capture_ntfs(
            manifest(vec![root(), regular, link, app, junction]),
            "oracle",
            &VolumeCaptureOptions {
                volume_aliases: vec!["\\??\\C:".into()],
                ..VolumeCaptureOptions::default()
            },
        )
        .unwrap();
    let output = fixture.0.join("oracle.wim");
    image.write(&output).unwrap();
    let verification = std::process::Command::new("wimlib-imagex")
        .arg("verify")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        verification.status.success(),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    assert!(
        verification.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    let listing = std::process::Command::new("wimlib-imagex")
        .arg("dir")
        .arg(&output)
        .args(["1", "--detailed"])
        .output()
        .unwrap();
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    assert!(
        listing.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let text = String::from_utf8_lossy(&listing.stdout);
    assert!(text.contains("owned-named"), "{text}");
    assert!(text.to_ascii_lowercase().contains("0x8000001b"), "{text}");
    assert!(
        text.contains("0xa000000c") || text.contains("0xA000000C"),
        "{text}"
    );
}

#[test]
fn absolute_links_require_volume_evidence_or_explicit_external_preservation() {
    let fixture = Fixture::new();
    let target = "\\??\\C:\\Windows\\target.txt";
    let make = || {
        let mut link = node(Some(0), "absolute", 93, 0x420);
        link.reparse_data = relative_link(target);
        link.reparse_data[16..20].fill(0); // absolute symlink
        manifest(vec![root(), link])
    };
    let mut image = Wim::new(Compression::None).unwrap();
    assert!(
        image
            .capture_ntfs(make(), "unproven", &VolumeCaptureOptions::default())
            .is_err()
    );
    assert_eq!(image.info().unwrap().image_count, 0);
    image
        .capture_ntfs(
            make(),
            "internal",
            &VolumeCaptureOptions {
                volume_aliases: vec!["\\??\\C:".into()],
                preserve_external_links: false,
                ..VolumeCaptureOptions::default()
            },
        )
        .unwrap();
    image
        .capture_ntfs(
            make(),
            "external",
            &VolumeCaptureOptions {
                volume_aliases: Vec::new(),
                preserve_external_links: true,
                ..VolumeCaptureOptions::default()
            },
        )
        .unwrap();
    image
        .capture_ntfs(
            make(),
            "setup",
            &VolumeCaptureOptions {
                volume_aliases: vec![r"\??\C:".into()],
                installation_system_drive: Some("C:".into()),
                ..VolumeCaptureOptions::default()
            },
        )
        .unwrap();
    for options in [
        VolumeCaptureOptions {
            installation_system_drive: Some("C:".into()),
            ..Default::default()
        },
        VolumeCaptureOptions {
            installation_system_drive: Some("D:".into()),
            volume_aliases: vec![r"\??\D:".into()],
            ..Default::default()
        },
        VolumeCaptureOptions {
            installation_system_drive: Some("C:".into()),
            volume_aliases: vec![r"\??\C:".into()],
            preserve_external_links: true,
        },
    ] {
        assert!(
            image
                .capture_ntfs(make(), "invalid-setup", &options)
                .is_err()
        );
        assert_eq!(image.info().unwrap().image_count, 3);
    }
    let output = fixture.0.join("absolute.wim");
    image.write(&output).unwrap();
    let bytes = std::fs::read(output).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    for (index, expected, flags) in [
        (1, "\\??\\X:\\Windows\\target.txt", 0),
        (2, target, 1),
        (3, target, 1),
    ] {
        let raw = archive.read_metadata(index).unwrap();
        let metadata = Metadata::parse(&raw).unwrap();
        let entry = &metadata.nodes[entry_index(&metadata, &units("absolute"))].entry;
        assert_eq!((entry.inode_union >> 48) & 1, flags);
        let rp = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::ReparsePoint)
            .unwrap();
        let payload = archive.read_blob(&rp.hash).unwrap();
        let substitute_offset = u16::from_le_bytes([payload[0], payload[1]]) as usize;
        let substitute_size = u16::from_le_bytes([payload[2], payload[3]]) as usize;
        assert_eq!(
            &payload[12 + substitute_offset..12 + substitute_offset + substitute_size],
            raw_units(&units(expected))
        );
    }
}

#[test]
fn duplicate_streams_invalid_names_and_divergent_hardlink_data_fail_closed() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut invalid = Vec::new();
    for name in ["", ".", "..", "a/b", "a\\b", "a:ads", "nul\0name"] {
        invalid.push(manifest(vec![root(), node(Some(0), name, 66, 0x20)]));
    }
    for stream_name in [Vec::new(), units("ads")] {
        let mut file = node(Some(0), "duplicate", 67, 0x20);
        file.streams = vec![
            stream(&stream_name, data(b"first", &reads)),
            stream(&stream_name, data(b"other", &reads)),
        ];
        invalid.push(manifest(vec![root(), file]));
    }
    let mut first = node(Some(0), "first", 68, 0x20);
    first.streams.push(stream(&[], data(b"first", &reads)));
    let mut alias = node(Some(0), "alias", 68, 0x20);
    alias.streams.push(stream(&[], data(b"other", &reads)));
    invalid.push(manifest(vec![root(), first, alias]));
    let mut sd = root();
    sd.security_descriptor = vec![0x77; 20];
    invalid.push(manifest(vec![sd]));
    let mut sd = root();
    sd.security_descriptor[16..20].copy_from_slice(&4096u32.to_le_bytes());
    invalid.push(manifest(vec![sd]));
    let mut image = Wim::new(Compression::None).unwrap();
    for manifest in invalid {
        assert!(
            image
                .capture_ntfs(manifest, "invalid", &VolumeCaptureOptions::default())
                .is_err()
        );
        assert_eq!(image.info().unwrap().image_count, 0);
    }
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "validation must not read source payloads"
    );
}

#[test]
fn encrypted_nodes_case_collisions_and_invalid_volume_aliases_fail_closed() {
    let mut image = Wim::new(Compression::None).unwrap();
    let mut encrypted = node(Some(0), "encrypted", 97, 0x4020);
    encrypted.security_descriptor = descriptor();
    assert!(
        image
            .capture_ntfs(
                manifest(vec![root(), encrypted]),
                "encrypted",
                &VolumeCaptureOptions::default()
            )
            .is_err()
    );
    for names in [("same", "same"), ("same", "SAME")] {
        assert!(
            image
                .capture_ntfs(
                    manifest(vec![
                        root(),
                        node(Some(0), names.0, 98, 0x20),
                        node(Some(0), names.1, 99, 0x20)
                    ]),
                    "collision",
                    &VolumeCaptureOptions::default()
                )
                .is_err()
        );
    }
    for alias in [
        "",
        "C:",
        "\\??\\C:\\",
        "\\??\\C:\\Windows",
        "\\??\\Volume{not-a-guid}",
        "\\??\\C:\0",
    ] {
        assert!(
            image
                .capture_ntfs(
                    manifest(vec![root()]),
                    "alias",
                    &VolumeCaptureOptions {
                        volume_aliases: vec![alias.into()],
                        preserve_external_links: true,
                        ..VolumeCaptureOptions::default()
                    }
                )
                .is_err(),
            "alias {alias:?}"
        );
    }
    assert_eq!(image.info().unwrap().image_count, 0);
}

#[test]
#[ignore = "requires independent mkntfs, ntfscp, and wimlib-imagex fixture tools"]
fn raw_ntfs_volume_captures_through_wim_with_independent_stream_bytes() {
    use std::process::Command;
    use windows_disk::RawDisk;
    use windows_ntfs::Volume;
    let fixture = Fixture::new();
    let input = fixture.0.join("raw-volume.ntfs");
    std::fs::File::create(&input)
        .unwrap()
        .set_len(32 * 1024 * 1024)
        .unwrap();
    let payload = fixture.0.join("payload");
    let expected: Vec<u8> = (0..131_073).map(|i| (i * 31) as u8).collect();
    std::fs::write(&payload, &expected).unwrap();
    let ads = fixture.0.join("ads");
    std::fs::write(&ads, b"independent named data").unwrap();
    for (program, args) in [
        (
            "mkntfs",
            vec!["-F", "-Q", "-c", "512", input.to_str().unwrap()],
        ),
        (
            "ntfscp",
            vec![
                input.to_str().unwrap(),
                payload.to_str().unwrap(),
                "/payload.bin",
            ],
        ),
        (
            "ntfscp",
            vec![
                "-N",
                "note",
                input.to_str().unwrap(),
                ads.to_str().unwrap(),
                "/payload.bin",
            ],
        ),
    ] {
        let result = Command::new(program).args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{program}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    use sha1::Digest;
    let before = std::fs::read(&input).unwrap();
    let before_digest = sha1::Sha1::digest(&before);
    let volume = Volume::open(Arc::new(RawDisk::open(&input).unwrap())).unwrap();
    let manifest = volume.capture_manifest().unwrap();
    let captured = manifest
        .nodes
        .iter()
        .find(|n| n.name == units("payload.bin"))
        .unwrap();
    let attributes = captured.attributes;
    let security = captured.security_descriptor.clone();
    let written = captured.write_time;
    let mut wim = Wim::new(Compression::Lzx).unwrap();
    wim.capture_ntfs(manifest, "raw volume", &VolumeCaptureOptions::default())
        .unwrap();
    drop(volume); // Deferred readers in the graph must retain the disk handle.
    let output = fixture.0.join("raw-volume.wim");
    wim.write(&output).unwrap();
    Wim::open(
        &output,
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
    )
    .unwrap()
    .verify()
    .unwrap();
    let bytes = std::fs::read(&output).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    let raw = archive.read_metadata(1).unwrap();
    let metadata = Metadata::parse(&raw).unwrap();
    let index = entry_index(&metadata, &units("payload.bin"));
    let entry = metadata.inode_entry(index).unwrap();
    assert_eq!(entry.attributes, attributes);
    assert_eq!(entry.last_write_time, written);
    assert_eq!(
        metadata.security_descriptor(index),
        Some(security.as_slice())
    );
    for (name, expected) in [
        (vec![], expected.as_slice()),
        (
            raw_units(&units("note")),
            b"independent named data".as_slice(),
        ),
    ] {
        let stream = entry
            .streams
            .iter()
            .find(|s| s.kind == StreamType::Data && s.name == name)
            .unwrap();
        assert_eq!(archive.read_blob(&stream.hash).unwrap(), expected);
    }
    let upstream = std::env::var_os("WIMLIB_IMAGEX").unwrap_or_else(|| "wimlib-imagex".into());
    let result = Command::new(upstream)
        .arg("verify")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "upstream WIM verify failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!String::from_utf8_lossy(&result.stderr).contains("WARNING"));
    let after_digest = sha1::Sha1::digest(std::fs::read(&input).unwrap());
    assert_eq!(before_digest, after_digest);
    println!(
        "independent raw NTFS input SHA-1 before/after: {before_digest:x}; exact bytes unchanged; upstream WIM verification passed"
    );
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "native capture mutated independent raw input"
    );
}

struct SparseData {
    bytes: Vec<u8>,
    holes: Vec<(u64, u64)>,
}
impl ReadAt for SparseData {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let end = start
            .checked_add(destination.len())
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        destination.copy_from_slice(
            self.bytes
                .get(start..end)
                .ok_or(io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
    fn sparse_holes(&self) -> io::Result<Vec<(u64, u64)>> {
        Ok(self.holes.clone())
    }
}

#[test]
fn sparse_holes_and_storage_class_survive_wim_serialization() {
    let fixture = Fixture::new();
    let mut file = node(Some(0), "sparse", 70, 0x220);
    file.storage_class = Some(2);
    let bytes = [vec![42; 8], vec![0; 48], vec![43; 8]].concat();
    file.streams = vec![stream(
        &[],
        Arc::new(SparseData {
            bytes: bytes.clone(),
            holes: vec![(8, 56)],
        }),
    )];
    let mut wim = Wim::new(Compression::None).unwrap();
    let (_, audit) = wim
        .capture_ntfs_with_audit(
            manifest(vec![root(), file]),
            "sparse",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    assert_eq!(audit.storage_class_nodes, 1);
    assert_eq!(audit.sparse_files, 1);
    assert_eq!(audit.sparse_hole_ranges, 1);
    let output = fixture.0.join("sparse.wim");
    wim.write(&output).unwrap();
    Wim::open(&output, OpenOptions::default())
        .unwrap()
        .verify()
        .unwrap();
    let raw = std::fs::read(&output).unwrap();
    let archive = Archive::open(&raw).unwrap();
    let meta = archive.read_metadata(1).unwrap();
    let metadata = Metadata::parse(&meta).unwrap();
    let entry = metadata
        .inode_entry(entry_index(&metadata, &units("sparse")))
        .unwrap();
    assert_eq!(
        entry.tagged_item(3, 16).unwrap()[..16],
        [8u64.to_le_bytes(), 56u64.to_le_bytes()].concat()
    );
    assert_eq!(&entry.tagged_item(4, 4).unwrap()[..4], &2u32.to_le_bytes());
    assert_eq!(archive.read_blob(&entry.streams[0].hash).unwrap(), bytes);
}

#[test]
fn invalid_sparse_holes_are_rejected_before_writing() {
    for holes in [vec![(8, 8)], vec![(8, 65)], vec![(8, 40), (32, 48)]] {
        let mut file = node(Some(0), "invalid", 71, 0x220);
        file.streams = vec![stream(
            &[],
            Arc::new(SparseData {
                bytes: vec![0; 64],
                holes,
            }),
        )];
        let mut wim = Wim::new(Compression::None).unwrap();
        assert!(
            wim.capture_ntfs(
                manifest(vec![root(), file]),
                "invalid",
                &VolumeCaptureOptions::default()
            )
            .is_err()
        );
    }
}

#[test]
fn portable_kernel_ea_omission_is_reported_and_user_ea_remains() {
    let fixture = Fixture::new();
    let mut file = node(Some(0), "ea", 98, 0x20);
    let names = [b"$kErNeL.cache".as_slice(), b"$CI.CATALOGHINT"];
    for (index, name) in names.iter().enumerate() {
        let start = file.extended_attributes.len();
        file.extended_attributes.extend_from_slice(&[0; 8]);
        file.extended_attributes[start + 5] = name.len() as u8;
        file.extended_attributes[start + 6..start + 8].copy_from_slice(&1u16.to_le_bytes());
        file.extended_attributes.extend_from_slice(name);
        file.extended_attributes.extend_from_slice(&[0, 42]);
        if index == 0 {
            while !file.extended_attributes.len().is_multiple_of(4) {
                file.extended_attributes.push(0);
            }
            let next = file.extended_attributes.len() as u32;
            file.extended_attributes[start..start + 4].copy_from_slice(&next.to_le_bytes());
        }
    }
    let mut image = Wim::new(Compression::None).unwrap();
    let (_, audit) = image
        .capture_ntfs_with_audit(
            manifest(vec![root(), file]),
            "portable EA",
            &VolumeCaptureOptions::default(),
        )
        .unwrap();
    assert_eq!(audit.omitted_kernel_eas, 1);
    assert_eq!(audit.nodes_with_omitted_kernel_eas, 1);
    let output = fixture.0.join("ea.wim");
    image.write(&output).unwrap();
    let bytes = std::fs::read(output).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    let raw = archive.read_metadata(1).unwrap();
    let metadata = Metadata::parse(&raw).unwrap();
    let entry = metadata
        .inode_entry(entry_index(&metadata, &units("ea")))
        .unwrap();
    let packed = entry.tagged_item(2, 4).unwrap();
    assert!(
        packed
            .windows(b"$CI.CATALOGHINT".len())
            .any(|name| name == b"$CI.CATALOGHINT")
    );
    assert!(
        !packed
            .windows(8)
            .any(|name| name.eq_ignore_ascii_case(b"$KERNEL."))
    );
}
