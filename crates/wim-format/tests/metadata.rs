use wim_format::{
    ParseError,
    metadata::{Dentry, Metadata, SecurityTable, StreamType},
};
fn put(b: &mut [u8], o: usize, n: u64) {
    b[o..o + 8].copy_from_slice(&n.to_le_bytes());
}
fn entry(name: &str, attr: u32) -> Vec<u8> {
    let utf: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let len = (102 + utf.len() + if utf.is_empty() { 0 } else { 2 } + 7) & !7;
    let mut b = vec![0; len];
    put(&mut b, 0, len as u64);
    b[8..12].copy_from_slice(&attr.to_le_bytes());
    b[12..16].fill(255);
    b[100..102].copy_from_slice(&(utf.len() as u16).to_le_bytes());
    b[102..102 + utf.len()].copy_from_slice(&utf);
    b
}
fn tree(children: &[Vec<u8>]) -> Vec<u8> {
    let mut root = entry("", 16);
    put(&mut root, 16, 120);
    let mut b = vec![0; 8];
    b.extend(root);
    b.extend([0; 8]);
    for c in children {
        b.extend(c);
    }
    b.extend([0; 8]);
    b
}
#[test]
fn parses_original_c_metadata_losslessly() {
    let b = include_bytes!("fixtures/metadata-unix.bin");
    let m = Metadata::parse(b).unwrap();
    assert_eq!(m.nodes.len(), 5);
    assert_eq!(m.raw.as_ptr(), b.as_ptr());
    let names: Vec<_> = m
        .nodes
        .iter()
        .map(|n| {
            String::from_utf16(
                &n.entry
                    .name
                    .chunks_exact(2)
                    .map(|x| u16::from_le_bytes([x[0], x[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(names, ["", "alias", "link", "sub", "file"]);
    assert_eq!(
        m.nodes[1].entry.hard_link_group_id(),
        m.nodes[4].entry.hard_link_group_id()
    );
    assert!(m.nodes[1].entry.creation_time > 0);
    assert!(!m.nodes[1].entry.tagged_items.is_empty());
    assert!(
        m.nodes[2]
            .entry
            .streams
            .iter()
            .any(|s| s.kind == StreamType::ReparsePoint)
    );
}
#[test]
fn security_table_checks_sizes_and_preserves_opaque_descriptors() {
    let mut b = vec![0; 24];
    b[..4].copy_from_slice(&24u32.to_le_bytes());
    b[4..8].copy_from_slice(&1u32.to_le_bytes());
    put(&mut b, 8, 3);
    b[16..19].copy_from_slice(b"abc");
    assert_eq!(
        SecurityTable::parse(&b).unwrap().descriptors.as_slice(),
        [b"abc".as_slice()]
    );
    put(&mut b, 8, u64::MAX);
    assert_eq!(
        SecurityTable::parse(&b).unwrap_err(),
        ParseError::InvalidMetadataResource
    );
}
#[test]
fn filters_invalid_and_duplicate_names_and_orders_utf16() {
    let b = tree(&[
        entry("z", 0),
        entry("a", 0),
        entry("a", 0),
        entry("..", 0),
        entry("", 0),
        entry("nul\0x", 0),
    ]);
    let m = Metadata::parse(&b).unwrap();
    assert_eq!(m.nodes.len(), 3);
    assert_eq!(m.nodes[0].children.as_slice(), [2, 1]);
}
#[test]
fn rejects_cycles_without_recursive_stack_overflow() {
    let mut c = entry("cycle", 16);
    put(&mut c, 16, 120);
    let b = tree(&[c]);
    assert_eq!(
        Metadata::parse(&b).unwrap_err(),
        ParseError::InvalidMetadataResource
    );
}
#[test]
fn preserves_names_and_accepts_upstream_unchecked_terminator() {
    let mut b = entry("a", 0);
    b[104..106].copy_from_slice(&[9, 9]);
    let (e, _) = Dentry::parse(&b, 0).unwrap().unwrap();
    assert_eq!(e.name, [97, 0]);
    assert_eq!(e.raw, b);
}
#[test]
fn named_and_unnamed_stream_order_matches_upstream() {
    let mut b = entry("x", 0x400);
    b[96..98].copy_from_slice(&3u16.to_le_bytes());
    for name in ["", "ads", ""] {
        let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let len = (38 + name.len() + 7) & !7;
        let mut s = vec![0; len];
        put(&mut s, 0, len as u64);
        s[16] = 1;
        s[36..38].copy_from_slice(&(name.len() as u16).to_le_bytes());
        s[38..38 + name.len()].copy_from_slice(&name);
        b.extend(s);
    }
    let (e, _) = Dentry::parse(&b, 0).unwrap().unwrap();
    assert_eq!(
        e.streams.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            StreamType::Unknown,
            StreamType::ReparsePoint,
            StreamType::Data,
            StreamType::Data
        ]
    );
}
#[test]
fn truncation_and_overflow_never_panic() {
    let b = include_bytes!("fixtures/metadata-unix.bin");
    for n in 0..b.len() {
        let _ = Metadata::parse(&b[..n]);
    }
    let mut b = entry("x", 0);
    put(&mut b, 0, u64::MAX);
    assert!(Dentry::parse(&b, 0).is_err());
}
#[test]
fn hardlinks_merge_only_consistent_regular_file_data() {
    let mut a = entry("z", 0);
    put(&mut a, 88, 7);
    a[64] = 9;
    let mut b = entry("a", 0);
    put(&mut b, 88, 7);
    b[64] = 9;
    let mut c = entry("different", 0);
    put(&mut c, 88, 7);
    c[64] = 8;
    let mut d = entry("directory", 16);
    put(&mut d, 88, 7);
    d[64] = 9;
    let bytes = tree(&[a, b, c, d]);
    let m = Metadata::parse(&bytes).unwrap();
    assert_eq!(m.nodes[1].inode, 2);
    assert_eq!(m.nodes[2].inode, 2);
    assert_eq!(m.nodes[3].inode, 3);
    assert_eq!(m.nodes[4].inode, 4);
}
#[test]
fn resolves_tagged_items_and_tolerates_malformed_tail() {
    let b = include_bytes!("fixtures/metadata-unix.bin");
    let m = Metadata::parse(b).unwrap();
    assert_eq!(
        m.nodes[1].entry.tagged_item(0x337dd873, 16).unwrap().len(),
        16
    );
    let mut b = entry("", 16);
    let len = b.len() + 8;
    b.resize(len, 0);
    put(&mut b, 0, len as u64);
    let last = b.len() - 4;
    b[last..].fill(255);
    let (e, _) = Dentry::parse(&b, 0).unwrap().unwrap();
    assert_eq!(e.tagged_item(1, 0), None);
}
#[test]
fn encrypted_stream_and_empty_images_match_upstream_rules() {
    let mut b = entry("", 0x4000);
    b[64] = 1;
    assert_eq!(
        Dentry::parse(&b, 0).unwrap().unwrap().0.streams[0].kind,
        StreamType::EncryptedRaw
    );
    let b = [0; 16];
    assert!(Metadata::parse(&b).unwrap().nodes.is_empty());
}
