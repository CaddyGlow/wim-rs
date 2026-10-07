//! In-memory split/join interoperability driver; no public CLI compatibility.
use wim_format::{
    Compression,
    repack::WriteOptions,
    split_join::{join_archives, split_archive},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("split") {
        let bytes = std::fs::read(&args[2])?;
        let size = args[3].parse()?;
        let parts = split_archive(&bytes, size, [0x51; 16], true)?;
        for (i, part) in parts.iter().enumerate() {
            std::fs::write(format!("{}{}.swm", args[4], i + 1), part)?;
        }
        println!("{}", parts.len());
    } else if args.get(1).map(String::as_str) == Some("join") {
        let parts: Vec<Vec<u8>> = args[3..]
            .iter()
            .map(std::fs::read)
            .collect::<Result<_, _>>()?;
        let references: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let bytes = join_archives(
            &references,
            WriteOptions {
                compression: Compression::Lzx,
                chunk_size: 32768,
                integrity: true,
            },
        )?;
        std::fs::write(&args[2], bytes)?;
    } else {
        return Err("usage: split INPUT TARGET PREFIX | join OUTPUT PARTS...".into());
    }
    Ok(())
}
