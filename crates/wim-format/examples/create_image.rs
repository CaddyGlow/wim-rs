//! Test-only deterministic new-image interoperability producer.
//! The fixed GUID is an explicitly chosen test identity, not random generation.
use wim_format::{
    Compression,
    image_build::{ImageBuilder, NewImage},
    metadata_write::{OwnedDentry, OwnedMetadata, OwnedStream},
    repack::WriteOptions,
};
fn name(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: create_image OUTPUT none|xpress|lzx|lzms INTEGRITY IMAGES".into());
    }
    let compression = match args[2].as_str() {
        "none" => Compression::None,
        "xpress" => Compression::Xpress,
        "lzx" => Compression::Lzx,
        "lzms" => Compression::Lzms,
        _ => return Err("unknown compression".into()),
    };
    let integrity: bool = args[3].parse()?;
    let count: u32 = args[4].parse()?;
    let mut builder = ImageBuilder::new([0x43; 16]);
    let content: Vec<_> = (0..153_618).map(|i| (i % 256) as u8).collect();
    let hash = builder.add_blob(&content)?;
    let stream = builder.add_blob(b"named stream content\n")?;
    for index in 0..count {
        let mut root = OwnedDentry::new(Vec::new(), 0x10);
        root.children = vec![1, 2, 3];
        let mut file = OwnedDentry::new(name("file.bin"), 0x80);
        file.main_hash = hash;
        file.inode_union = 17;
        file.security_id = 0;
        file.extra_streams.push(OwnedStream {
            hash: stream,
            name: name("extra"),
            ..Default::default()
        });
        let mut alias = file.clone();
        alias.name = name("alias.bin");
        let directory = OwnedDentry::new(name("directory"), 0x10);
        let mut sd = vec![0; 20];
        sd[0] = 1;
        sd[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        builder.add_image(NewImage {
            metadata: OwnedMetadata {
                security_descriptors: vec![sd],
                nodes: vec![root, file, alias, directory],
            },
            name: Some(format!("native {}", index + 1)),
            description: Some("Built without an existing WIM".into()),
            properties: vec![("WINDOWS/VERSION/BUILD".into(), "12345".into())],
        })?;
    }
    if count > 0 {
        builder.set_boot_index(count)?;
    }
    let bytes = builder.write(WriteOptions {
        compression,
        chunk_size: if compression == Compression::None {
            0
        } else {
            32768
        },
        integrity,
    })?;
    std::fs::write(&args[1], bytes)?;
    Ok(())
}
