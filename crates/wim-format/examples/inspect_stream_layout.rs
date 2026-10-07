//! Inspect raw WIM stream references without assuming Microsoft accepts them.
use wim_format::{file_archive::FileArchive, metadata::Metadata};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: inspect_stream_layout WIM BASENAME".into());
    }
    let archive = FileArchive::open(std::fs::File::open(&args[1])?)?;
    let bytes = archive.read_metadata(1)?;
    let metadata = Metadata::parse(&bytes)?;
    let name: Vec<_> = args[2].encode_utf16().flat_map(u16::to_le_bytes).collect();
    for node in metadata.nodes.iter().filter(|n| n.entry.name == name) {
        let entry = &node.entry;
        let offset = entry.offset;
        println!(
            "offset={offset} attributes={:#x} union={} main_hash={}",
            entry.attributes,
            hex(&bytes[offset + 88..offset + 96]),
            hex(&bytes[offset + 64..offset + 84])
        );
        let count = u16::from_le_bytes(bytes[offset + 96..offset + 98].try_into()?);
        let mut slot = offset + u64::from_le_bytes(bytes[offset..offset + 8].try_into()?) as usize;
        for i in 0..count {
            let length = u64::from_le_bytes(bytes[slot..slot + 8].try_into()?) as usize;
            let name_bytes = u16::from_le_bytes(bytes[slot + 36..slot + 38].try_into()?) as usize;
            println!(
                "extra[{i}] name={} hash={}",
                hex(&bytes[slot + 38..slot + 38 + name_bytes]),
                hex(&bytes[slot + 16..slot + 36])
            );
            slot += length;
        }
        for stream in &entry.streams {
            println!(
                "classified {:?} name={} hash={}",
                stream.kind,
                hex(stream.name),
                hex(&stream.hash)
            );
            if stream.hash != [0; 20] {
                let data = archive.read_blob(&stream.hash)?;
                println!(
                    "resource_length={} prefix={}",
                    data.len(),
                    hex(&data[..data.len().min(64)])
                );
            }
        }
    }
    Ok(())
}
