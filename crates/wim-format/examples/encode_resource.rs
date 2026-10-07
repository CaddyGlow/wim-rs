//! Test-only resource serializer driver for the original reader oracle.
use std::{error::Error, fs, path::PathBuf};
use wim_format::{Compression, resource::ResourceLayout, resource_write::encode_resource};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 6 {
        return Err("INPUT CODEC CHUNK LAYOUT CHUNKDIR OUTPUT".into());
    }
    let input = fs::read(&args[0])?;
    let compression = Compression::from_i32(args[1].parse()?)?;
    let chunk_size = args[2].parse()?;
    let layout = match args[3].as_str() {
        "ordinary" => ResourceLayout::Ordinary,
        "pipable" => ResourceLayout::Pipable,
        "solid" => ResourceLayout::Solid,
        _ => return Err("invalid layout".into()),
    };
    let mut index = 0;
    let resource = encode_resource(&input, compression, chunk_size, layout, |_, _| {
        let path = PathBuf::from(&args[4]).join(index.to_string());
        index += 1;
        fs::read(path)
            .map(|bytes| if bytes.is_empty() { None } else { Some(bytes) })
            .map_err(|_| wim_format::ParseError::Read)
    })?;
    fs::write(&args[5], &resource.bytes)?;
    fs::write(format!("{}.descriptor", args[5]), resource.header.encode())?;
    Ok(())
}
