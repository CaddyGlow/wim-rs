//! Export images into an existing archive using native resources.
use wim_format::{Compression, archive::Archive, image_ops::export_images, repack::WriteOptions};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: image_export DESTINATION SOURCE OUTPUT 1,2,...".into());
    }
    let dest = std::fs::read(&args[1])?;
    let source = std::fs::read(&args[2])?;
    let indices: Vec<u32> = args[4]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    let bytes = export_images(
        &Archive::open(&dest)?,
        &Archive::open(&source)?,
        &indices,
        WriteOptions {
            compression: Compression::Xpress,
            chunk_size: 32768,
            integrity: true,
        },
    )?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    std::io::Write::write_all(&mut file, &bytes)?;
    Ok(())
}
