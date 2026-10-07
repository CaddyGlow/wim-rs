//! Read a hash-identified blob byte range using native bounded chunk decoding.
use std::{env, fs};
use wim_format::archive::Archive;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 6 {
        return Err("usage: read_range WIM SHA1 OFFSET LENGTH OUTPUT".into());
    }
    if args[2].len() != 40 || !args[2].is_ascii() {
        return Err("invalid SHA1".into());
    }
    let mut hash = [0; 20];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&args[2][i * 2..i * 2 + 2], 16)?;
    }
    let start: u64 = args[3].parse()?;
    let size: u64 = args[4].parse()?;
    let end = start.checked_add(size).ok_or("range overflow")?;
    let bytes = fs::read(&args[1])?;
    let archive = Archive::open(&bytes)?;
    let output = archive.read_blob_range(&hash, start..end)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[5])?;
    std::io::Write::write_all(&mut file, &output)?;
    Ok(())
}
