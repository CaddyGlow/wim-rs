use wim_format::{metadata::Metadata, metadata_write::OwnedMetadata};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: rewrite_metadata INPUT OUTPUT".into());
    }
    let bytes = std::fs::read(&args[1])?;
    let metadata = Metadata::parse(&bytes)?;
    std::fs::write(&args[2], OwnedMetadata::from_metadata(&metadata)?.encode()?)?;
    Ok(())
}
