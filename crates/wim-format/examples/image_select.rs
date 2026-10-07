//! Select/reorder native archive images for original-library comparison.
use wim_format::{Compression, archive::Archive, image_ops::select_images, repack::WriteOptions};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: image_select INPUT OUTPUT 1,2,...".into());
    }
    let source = std::fs::read(&args[1])?;
    let archive = Archive::open(&source)?;
    let images: Vec<u32> = if args[3] == "-" {
        Vec::new()
    } else {
        args[3]
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()?
    };
    let output = select_images(
        &archive,
        &images,
        WriteOptions {
            compression: Compression::Xpress,
            chunk_size: 32768,
            integrity: true,
        },
    )?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    std::io::Write::write_all(&mut file, &output)?;
    Ok(())
}
