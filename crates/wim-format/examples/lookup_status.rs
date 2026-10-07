//! Differential probe for lookup resolution; not an implementation of open_wim.

use std::{env, fs, process::ExitCode};
use wim_format::{
    Compression, Header, ParseError,
    lookup::LookupTable,
    resource::{ResourceLayout, read_resource},
};

fn probe(path: &str) -> Result<LookupTable, ParseError> {
    let file = fs::read(path).map_err(|_| ParseError::Read)?;
    let header = Header::parse_seekable(&file)?;
    let compression = header.validate_compression()?;
    let bytes = read_resource(
        &file,
        &header.blob_table,
        compression,
        header.chunk_size,
        if header.magic == wim_format::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        },
        |kind, input, output| {
            let accepted = match kind {
                Compression::Xpress => ms_compress::decompress_xpress(input, output).is_ok(),
                Compression::Lzx => {
                    ms_compress::lzx::decompress_lzx(input, output, header.chunk_size as usize)
                        .is_ok()
                }
                Compression::Lzms => ms_compress::lzms::decompress_lzms(input, output).is_ok(),
                Compression::None => false,
            };
            if accepted {
                Ok(())
            } else {
                Err(ParseError::Decompression)
            }
        },
    )?;
    LookupTable::parse(&bytes, &header, |offset| {
        let start = usize::try_from(offset).map_err(|_| ParseError::UnexpectedEndOfFile)?;
        let end = start
            .checked_add(16)
            .ok_or(ParseError::UnexpectedEndOfFile)?;
        let source = file
            .get(start..end)
            .ok_or(ParseError::UnexpectedEndOfFile)?;
        let mut bytes = [0; 16];
        bytes.copy_from_slice(source);
        Ok(bytes)
    })
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: lookup_status PATH");
        return ExitCode::FAILURE;
    };
    match probe(&path) {
        Err(error) => println!("STATUS {}", error.as_i32()),
        Ok(table) => {
            println!("STATUS 0 {}", table.effective_image_count);
            for (metadata, blob) in table
                .metadata
                .iter()
                .map(|b| (true, b))
                .chain(table.blobs.iter().map(|b| (false, b)))
            {
                let resource = &table.resources[blob.resource_index];
                let hash: String = blob.hash.iter().map(|b| format!("{b:02x}")).collect();
                let flags = (resource.header.flags & 0x1d) | if metadata { 2 } else { 0 };
                println!(
                    "ENTRY {hash} {} {} {flags} {} {} {} {}",
                    blob.size,
                    blob.reference_count,
                    if resource.solid {
                        blob.offset
                    } else {
                        resource.header.offset_in_wim
                    },
                    resource.header.offset_in_wim,
                    resource.header.size_in_wim,
                    resource.uncompressed_size
                );
            }
        }
    }
    ExitCode::SUCCESS
}
