use wim_format::{
    ParseError,
    metadata::Metadata,
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream, write_lossless},
};
fn name(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn empty_directories_have_terminated_child_lists_and_files_have_none() {
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 2];
    let tree = OwnedMetadata {
        security_descriptors: vec![],
        nodes: vec![
            root,
            OwnedDentry::new(name("empty-directory"), 0x10),
            OwnedDentry::new(name("empty-file"), 0x20),
        ],
    };
    let bytes = tree.encode().unwrap();
    let metadata = Metadata::parse(&bytes).unwrap();
    for node in &metadata.nodes {
        let offset = node.entry.subdir_offset as usize;
        if node.entry.attributes & 0x10 != 0 {
            assert_ne!(offset, 0);
            if node.children.is_empty() {
                assert_eq!(&bytes[offset..offset + 8], &[0; 8]);
            }
        } else {
            assert_eq!(offset, 0);
        }
    }
    let bytes = OwnedMetadata {
        security_descriptors: vec![],
        nodes: vec![OwnedDentry::new(Vec::new(), 0x10)],
    }
    .encode()
    .unwrap();
    assert_eq!(
        Metadata::parse(&bytes).unwrap().nodes[0]
            .entry
            .subdir_offset,
        0
    );
}
#[test]
fn directory_reparse_points_have_no_child_list_and_cannot_contain_children() {
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 2];
    let mut junction = OwnedDentry::new(name("junction"), 0x410);
    junction.inode_union = 0xa0000003;
    let mut tree = OwnedMetadata {
        security_descriptors: vec![],
        nodes: vec![root, junction, OwnedDentry::new(name("ordinary"), 0x10)],
    };
    let bytes = tree.encode().unwrap();
    let parsed = Metadata::parse(&bytes).unwrap();
    let junction = parsed
        .nodes
        .iter()
        .find(|n| n.entry.name == name("junction"))
        .unwrap();
    assert_eq!(junction.entry.subdir_offset, 0);
    assert!(junction.children.is_empty());
    let ordinary = parsed
        .nodes
        .iter()
        .find(|n| n.entry.name == name("ordinary"))
        .unwrap();
    assert_ne!(ordinary.entry.subdir_offset, 0);
    let offset = ordinary.entry.subdir_offset as usize;
    assert_eq!(&bytes[offset..offset + 8], &[0; 8]);
    let junction_offset = junction.entry.offset;
    let mut malformed = bytes.clone();
    malformed[junction_offset + 16..junction_offset + 24]
        .copy_from_slice(&(offset as u64).to_le_bytes());
    assert!(matches!(
        Metadata::parse(&malformed),
        Err(ParseError::InvalidMetadataResource)
    ));

    tree.nodes[0].children = vec![1];
    tree.nodes[1].children = vec![2];
    assert!(matches!(
        tree.encode(),
        Err(ParseError::InvalidMetadataResource)
    ));
}

fn tree() -> OwnedMetadata {
    let mut root = OwnedDentry::new(Vec::new(), 16);
    root.children = vec![1, 2];
    let mut sub = OwnedDentry::new(name("sub"), 16);
    sub.children = vec![3];
    let mut file = OwnedDentry::new(name("file"), 128);
    file.main_hash = [3; 20];
    file.inode_union = 79;
    let mut alias = file.clone();
    alias.name = name("alias");
    OwnedMetadata {
        security_descriptors: vec![vec![1, 0, 4, 128, 0, 0, 0, 0]],
        nodes: vec![root, sub, alias, file],
    }
}
#[test]
fn original_metadata_lossless_write_preserves_every_byte() {
    let b = include_bytes!("fixtures/metadata-unix.bin");
    assert_eq!(write_lossless(&Metadata::parse(b).unwrap()).unwrap(), b);
}
#[test]
fn owned_original_metadata_preserves_visible_inode_fields() {
    let old = Metadata::parse(include_bytes!("fixtures/metadata-unix.bin")).unwrap();
    let b = OwnedMetadata::from_metadata(&old)
        .unwrap()
        .encode()
        .unwrap();
    let new = Metadata::parse(&b).unwrap();
    assert_eq!(new.nodes.len(), old.nodes.len());
    for (a, b) in old.nodes.iter().zip(&new.nodes) {
        assert_eq!(a.entry.name, b.entry.name);
        assert_eq!(a.entry.tagged_items, b.entry.tagged_items);
    }
}
#[test]
fn rebuilt_offsets_retain_nested_tree_and_hardlinks() {
    let b = tree().encode().unwrap();
    let m = Metadata::parse(&b).unwrap();
    assert_eq!(
        m.nodes
            .iter()
            .map(|n| n.entry.name.to_vec())
            .collect::<Vec<_>>(),
        vec![name(""), name("alias"), name("sub"), name("file")]
    );
    assert_eq!(m.nodes[3].inode, 1);
}
#[test]
fn windows_opaque_fields_survive_construction() {
    let mut m = tree();
    let n = &mut m.nodes[2];
    n.creation_time = 0xffff12345678;
    n.last_access_time = 19;
    n.last_write_time = 77;
    n.security_id = 0;
    n.reserved = [42; 16];
    n.unknown_0x54 = 123;
    n.short_name = name("ALIAS~1");
    n.tagged_items = vec![9, 0, 0, 0, 3, 0, 0, 0, 91, 92, 93, 0, 0, 0, 0, 0];
    n.extra_streams.push(OwnedStream {
        hash: [9; 20],
        name: vec![0, 216],
        reserved: [11; 8],
        trailing: vec![97, 98],
    });
    let b = m.encode().unwrap();
    let p = Metadata::parse(&b).unwrap();
    let n = &p.nodes[1].entry;
    assert_eq!(n.creation_time, 0xffff12345678);
    assert_eq!(n.short_name, name("ALIAS~1"));
    assert_eq!(n.tagged_item(9, 3), Some([91, 92, 93].as_slice()));
    assert_eq!(n.streams[1].name, [0, 216]);
    assert_eq!(
        p.security_descriptor(1),
        Some(m.security_descriptors[0].as_slice())
    );
}
#[test]
fn child_cycle_is_rejected() {
    let mut m = tree();
    m.nodes[1].children = vec![0];
    assert_eq!(m.encode(), Err(ParseError::InvalidMetadataResource));
}
#[test]
fn duplicate_names_are_rejected() {
    let mut m = tree();
    m.nodes[1].name = m.nodes[2].name.clone();
    assert_eq!(m.encode(), Err(ParseError::InvalidMetadataResource));
}
#[test]
fn unreachable_nodes_are_rejected() {
    let mut m = tree();
    m.nodes.push(OwnedDentry::new(name("orphan"), 128));
    assert_eq!(m.encode(), Err(ParseError::InvalidMetadataResource));
}
#[test]
fn odd_utf16_names_are_rejected() {
    let mut m = tree();
    m.nodes[1].name = vec![1];
    assert_eq!(m.encode(), Err(ParseError::InvalidMetadataResource));
}
#[test]
fn invalid_security_id_normalizes_when_imported() {
    let b = tree().encode().unwrap();
    let p = Metadata::parse(&b).unwrap();
    let owned = OwnedMetadata::from_metadata(&p).unwrap();
    assert_eq!(owned.nodes[1].security_id, u32::MAX);
}
#[test]
fn newly_constructed_named_stream_writes_upstream_terminator_and_alignment() {
    let mut m = tree();
    m.nodes[2].extra_streams.push(OwnedStream {
        name: name("x"),
        ..OwnedStream::default()
    });
    let b = m.encode().unwrap();
    let p = Metadata::parse(&b).unwrap();
    assert_eq!(p.nodes[1].entry.streams[1].raw.len(), 48);
}
