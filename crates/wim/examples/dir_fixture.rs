//! Build opaque Windows metadata fixtures for independent C callback comparison.
use wim_format::{
    Compression, Header,
    image_build::{ImageBuilder, NewImage},
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
    repack::WriteOptions,
};
fn name(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn tag(output: &mut Vec<u8>, id: u32, body: &[u8]) {
    output.extend_from_slice(&id.to_le_bytes());
    output.extend_from_slice(&(body.len() as u32).to_le_bytes());
    output.extend_from_slice(body);
    while !output.len().is_multiple_of(8) {
        output.push(0)
    }
}
fn main() {
    let directory = std::env::args().nth(1).unwrap();
    let mut builder = ImageBuilder::new([5; 16]);
    let data = builder.add_blob(b"default data").unwrap();
    let named = builder.add_blob(b"named stream data").unwrap();
    let reparse = builder.add_blob(b"opaque reparse bytes").unwrap();
    let encrypted = builder.add_blob(b"opaque EFS raw bytes").unwrap();
    let mut root = OwnedDentry::new(Vec::new(), 16);
    root.children = vec![1, 2, 3, 4, 5];
    root.security_id = 0;
    let mut file = OwnedDentry::new(name("ads"), 128);
    file.main_hash = data;
    file.short_name = name("ADS~1");
    file.inode_union = 79;
    file.security_id = 0;
    file.creation_time = u64::MAX;
    file.last_write_time = 116_444_736_001_234_567;
    file.last_access_time = 7;
    tag(
        &mut file.tagged_items,
        0x337dd873,
        &[123u32, 456, 0o100640, 0x12345678]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    tag(&mut file.tagged_items, 1, &(1..=80).collect::<Vec<u8>>());
    file.extra_streams = vec![
        OwnedStream {
            hash: named,
            name: name("zone"),
            ..OwnedStream::default()
        },
        OwnedStream {
            hash: data,
            name: name("日本語"),
            ..OwnedStream::default()
        },
    ];
    let mut alias = file.clone();
    alias.name = name("alias");
    alias.short_name = name("ALIAS~1");
    alias.creation_time = 19;
    let mut point = OwnedDentry::new(name("reparse"), 0x480);
    point.main_hash = reparse;
    point.inode_union = 0xa000000c;
    point.extra_streams.push(OwnedStream {
        hash: data,
        ..OwnedStream::default()
    });
    let mut cipher = OwnedDentry::new(name("encrypted"), 0x4080);
    cipher.main_hash = encrypted;
    let mut object = OwnedDentry::new(name("object"), 128);
    tag(&mut object.tagged_items, 1, &[7; 20]);
    object.security_id = 100;
    let mut descriptor = vec![0u8; 20];
    descriptor[0] = 1;
    descriptor[3] = 128;
    let metadata = OwnedMetadata {
        security_descriptors: vec![descriptor],
        nodes: vec![root, file, alias, point, cipher, object],
    };
    builder
        .add_image(NewImage {
            metadata: metadata.clone(),
            name: Some("Opaque metadata".into()),
            properties: Vec::new(),
            description: None,
        })
        .unwrap();
    let options = WriteOptions {
        compression: Compression::None,
        chunk_size: 0,
        integrity: false,
    };
    let bytes = builder.write(options).unwrap();
    std::fs::write(format!("{directory}/rich.wim"), &bytes).unwrap();
    let header = Header::parse_seekable(&bytes).unwrap();
    let mut missing = bytes;
    for entry in missing[header.blob_table.offset_in_wim as usize..]
        [..header.blob_table.size_in_wim as usize]
        .chunks_exact_mut(50)
    {
        if entry[30..50] == data {
            entry[30..50].copy_from_slice(&[99; 20]);
        }
    }
    std::fs::write(format!("{directory}/missing.wim"), missing).unwrap();
    let mut invalid = ImageBuilder::new([6; 16]);
    let mut invalid_metadata = metadata;
    invalid_metadata.nodes.truncate(2);
    invalid_metadata.nodes[0].children = vec![1];
    let child = &mut invalid_metadata.nodes[1];
    child.name = vec![0, 0xd8];
    child.main_hash = [0; 20];
    child.extra_streams.clear();
    invalid
        .add_image(NewImage {
            metadata: invalid_metadata,
            name: None,
            properties: Vec::new(),
            description: None,
        })
        .unwrap();
    std::fs::write(
        format!("{directory}/invalid-utf16.wim"),
        invalid.write(options).unwrap(),
    )
    .unwrap();
}
