use wim_format::{
    Compression, ParseError,
    archive::Archive,
    image_build::{ImageBuilder, NewImage},
    metadata::Metadata,
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
    repack::WriteOptions,
};

fn name(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn options(compression: Compression) -> WriteOptions {
    WriteOptions {
        compression,
        chunk_size: if compression == Compression::None {
            0
        } else {
            32768
        },
        integrity: true,
    }
}
fn tree(builder: &mut ImageBuilder) -> NewImage {
    let hash = builder.add_blob(b"new archive content\n").unwrap();
    let named = builder.add_blob(b"named stream content\n").unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 0x10);
    root.children = vec![1, 2, 3];
    let mut first = OwnedDentry::new(name("first"), 0x80);
    first.main_hash = hash;
    first.inode_union = 55;
    first.extra_streams.push(OwnedStream {
        hash: named,
        name: name("named"),
        ..Default::default()
    });
    let mut alias = first.clone();
    alias.name = name("alias");
    let directory = OwnedDentry::new(name("directory"), 0x10);
    NewImage {
        metadata: OwnedMetadata {
            security_descriptors: vec![],
            nodes: vec![root, first, alias, directory],
        },
        name: Some("new & native".into()),
        description: Some("created from bytes".into()),
        properties: vec![("WINDOWS/VERSION/BUILD".into(), "12345".into())],
    }
}

#[test]
fn newly_built_images_preserve_streams_links_properties_and_boot() {
    let mut builder = ImageBuilder::new([0x42; 16]);
    let image = tree(&mut builder);
    let hash = image.metadata.nodes[1].main_hash;
    builder.add_blob(b"unused").unwrap();
    builder.add_image(image).unwrap();
    builder.set_boot_index(1).unwrap();
    for compression in [
        Compression::None,
        Compression::Xpress,
        Compression::Lzx,
        Compression::Lzms,
    ] {
        let bytes = builder.write(options(compression)).unwrap();
        let archive = Archive::open(&bytes).unwrap();
        assert_eq!(archive.header.guid, [0x42; 16]);
        assert_eq!(archive.header.boot_index, 1);
        assert_eq!(archive.lookup.blobs.len(), 2);
        assert_eq!(archive.lookup.find(&hash).unwrap().reference_count, 2);
        assert_eq!(archive.read_blob(&hash).unwrap(), b"new archive content\n");
        let raw = archive.read_metadata(1).unwrap();
        let metadata = Metadata::parse(&raw).unwrap();
        assert_eq!(metadata.nodes[1].inode, metadata.nodes[3].inode);
        let xml = archive.xml().unwrap();
        assert_eq!(xml.name(1), Some("new & native"));
        assert_eq!(xml.get_property(1, "HARDLINKBYTES"), Some("41"));
        assert_eq!(xml.get_property(1, "DIRCOUNT"), Some("2"));
        assert_eq!(xml.get_property(1, "FILECOUNT"), Some("2"));
    }
}

#[test]
fn zero_image_archive_is_writable_and_openable() {
    let builder = ImageBuilder::new([7; 16]);
    let bytes = builder.write(options(Compression::None)).unwrap();
    let archive = Archive::open(&bytes).unwrap();
    assert_eq!(archive.header.image_count, 0);
    assert_eq!(archive.xml().unwrap().image_count(), 0);
}

#[test]
fn rejects_duplicate_nonempty_names_missing_blobs_and_invalid_boot_selection() {
    let mut builder = ImageBuilder::new([0; 16]);
    let image = tree(&mut builder);
    builder.add_image(image.clone()).unwrap();
    assert_eq!(
        builder.add_image(image.clone()),
        Err(ParseError::ImageNameCollision)
    );
    assert_eq!(builder.set_boot_index(2), Err(ParseError::InvalidImage));
    let mut missing = ImageBuilder::new([0; 16]);
    missing.add_image(image).unwrap();
    assert_eq!(
        missing.write(options(Compression::None)),
        Err(ParseError::ResourceNotFound)
    );
}
