//! Capture an immutable Windows-created unpaired UTF16/extension-record fixture.
#[cfg(feature = "disk-capture")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use partmgr::partition::PartitionTable;
    use std::{io, sync::Arc};
    use virtdisk::RawDisk;
    use wim::{Compression, OpenOptions, VolumeCaptureOptions, Wim};
    use wim_format::{
        archive::Archive,
        metadata::{Metadata, StreamType},
    };
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(io::Error::other("usage: utf16_disk_fixture FIXED_VHD OUTPUT_WIM").into());
    }
    if std::path::Path::new(&args[2]).exists() {
        return Err(io::Error::other("output already exists").into());
    }
    let table = PartitionTable::read(Arc::new(RawDisk::open(&args[1])?), 512)?;
    let volume = disk_capture::Volume::open(Arc::new(table.select(1)?))?;
    let root: Vec<u16> = "OddFixture".encode_utf16().collect();
    let manifest = volume.capture_manifest_with_filter(|path| path[0] == root)?;
    let mut image = Wim::new(Compression::Lzx)?;
    image.capture_ntfs(
        manifest,
        "UTF16 and attribute-list fixture",
        &VolumeCaptureOptions::default(),
    )?;
    image.write(std::path::Path::new(&args[2]))?;
    let mut reopened = Wim::open(
        std::path::Path::new(&args[2]),
        OpenOptions {
            check_integrity: true,
            ..Default::default()
        },
    )?;
    reopened.verify()?;
    let bytes = std::fs::read(&args[2])?;
    let archive = Archive::open(&bytes)?;
    let raw = archive.read_metadata(1)?;
    let metadata = Metadata::parse(&raw)?;
    let mut name: Vec<u16> = "unpaired-".encode_utf16().collect();
    name.push(0xd800);
    name.extend(".bin".encode_utf16());
    let raw_name: Vec<u8> = name.into_iter().flat_map(u16::to_le_bytes).collect();
    let odd = metadata
        .nodes
        .iter()
        .position(|node| node.entry.name == raw_name)
        .ok_or_else(|| io::Error::other("WIM changed unpaired UTF16 name"))?;
    let stream = metadata
        .inode_entry(odd)
        .unwrap()
        .streams
        .iter()
        .find(|stream| stream.kind == StreamType::Data && stream.name.is_empty())
        .ok_or_else(|| io::Error::other("WIM lost unpaired file data"))?;
    if archive.read_blob(&stream.hash)? != [42] {
        return Err(io::Error::other("unpaired WIM data mismatch").into());
    }
    let many_name: Vec<u8> = "many-ads.bin"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let many = metadata
        .nodes
        .iter()
        .position(|node| node.entry.name == many_name)
        .ok_or_else(|| io::Error::other("WIM lost extension-record file"))?;
    let streams = &metadata.inode_entry(many).unwrap().streams;
    if streams.len() != 161 {
        return Err(io::Error::other("WIM lost attribute-list streams").into());
    }
    for i in 0u16..160 {
        let name: Vec<u8> = format!("stream-{i}")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let stream = streams
            .iter()
            .find(|stream| stream.kind == StreamType::Data && stream.name == name)
            .ok_or_else(|| io::Error::other("WIM lost named stream"))?;
        if archive.read_blob(&stream.hash)? != [i as u8] {
            return Err(io::Error::other("WIM changed stream data").into());
        }
    }
    println!(
        "verified exact unpaired UTF16 filename/payload and all 160 attribute-list ADS in reopened WIM"
    );
    Ok(())
}
#[cfg(not(feature = "disk-capture"))]
fn main() {
    eprintln!("requires --features disk-capture");
    std::process::exit(1);
}
