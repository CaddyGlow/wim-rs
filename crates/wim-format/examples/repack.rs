//! Rewrite a seekable archive using the native uncompressed writer.
use std::{env, fs};
use wim_format::{
    Compression,
    archive::Archive,
    repack::{WriteOptions, write_archive},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 && args.len() != 5 {
        return Err(
            "usage: repack INPUT OUTPUT integrity|no-integrity [None|XPRESS|LZX|LZMS]".into(),
        );
    }
    let integrity = match args[3].as_str() {
        "integrity" => true,
        "no-integrity" => false,
        _ => return Err("unknown integrity mode".into()),
    };
    if args[1] == args[2] {
        return Err("input and output must differ".into());
    }
    let input = fs::read(&args[1])?;
    let archive = Archive::open(&input)?;
    let compression = match args.get(4).map(String::as_str).unwrap_or("None") {
        "None" => Compression::None,
        "XPRESS" => Compression::Xpress,
        "LZX" => Compression::Lzx,
        "LZMS" => Compression::Lzms,
        _ => return Err("unknown compression mode".into()),
    };
    let bytes = write_archive(
        &archive,
        WriteOptions {
            compression,
            chunk_size: if compression == Compression::None {
                0
            } else {
                32768
            },
            integrity,
        },
    )?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    std::io::Write::write_all(&mut file, &bytes)?;
    Ok(())
}
