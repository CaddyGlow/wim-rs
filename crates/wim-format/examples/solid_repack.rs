//! Write solid WIMs using the native buffered serializer.
use std::{env, fs, io::Write};
use wim_format::{
    Compression, archive::Archive, repack::WriteOptions, solid_write::write_solid_archive,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: solid_repack INPUT OUTPUT XPRESS|LZX|LZMS integrity|no-integrity".into(),
        );
    }
    let compression = match args[3].as_str() {
        "XPRESS" => Compression::Xpress,
        "LZX" => Compression::Lzx,
        "LZMS" => Compression::Lzms,
        _ => return Err("unknown codec".into()),
    };
    let integrity = match args[4].as_str() {
        "integrity" => true,
        "no-integrity" => false,
        _ => return Err("unknown integrity setting".into()),
    };
    let input = fs::read(&args[1])?;
    let source = Archive::open(&input)?;
    let bytes = write_solid_archive(
        &source,
        WriteOptions {
            compression,
            chunk_size: 32768,
            integrity,
        },
    )?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?
        .write_all(&bytes)?;
    Ok(())
}
