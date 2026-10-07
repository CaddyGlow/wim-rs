//! Probe file-backed resource ranges; only the lookup resource is loaded whole.
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
};
use wim_format::{
    Compression, HEADER_SIZE, Header, PIPABLE_MAGIC, ParseError,
    file_resource::read_resource_range, lookup::LookupTable, resource::ResourceLayout,
};
fn decode(kind: Compression, input: &[u8], output: &mut [u8]) -> Result<(), ParseError> {
    match kind {
        Compression::None => Err(ParseError::InvalidCompressionType),
        Compression::Xpress => {
            ms_compress::decompress_xpress(input, output).map_err(|_| ParseError::Decompression)
        }
        Compression::Lzx => {
            ms_compress::lzx::decompress_lzx(input, output, output.len().next_power_of_two())
                .map_err(|_| ParseError::Decompression)
        }
        Compression::Lzms => {
            ms_compress::lzms::decompress_lzms(input, output).map_err(|_| ParseError::Decompression)
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 6 {
        return Err("usage: read_file_range WIM SHA1 OFFSET LENGTH OUTPUT".into());
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
    let mut file = fs::File::open(&args[1])?;
    let file_size = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;
    let mut header_bytes = [0; HEADER_SIZE];
    file.read_exact(&mut header_bytes)?;
    let pipable = header_bytes[..8] == PIPABLE_MAGIC;
    if pipable {
        file.seek(SeekFrom::End(-(HEADER_SIZE as i64)))?;
        file.read_exact(&mut header_bytes)?;
        header_bytes[..8].copy_from_slice(&PIPABLE_MAGIC);
    }
    let header = Header::parse(&header_bytes, Some(file_size))?;
    let layout = if pipable {
        ResourceLayout::Pipable
    } else {
        ResourceLayout::Ordinary
    };
    let table_bytes = read_resource_range(
        &mut file,
        &header.blob_table,
        header.validate_compression()?,
        header.chunk_size,
        layout,
        0..header.blob_table.uncompressed_size,
        decode,
    )?;
    let table = LookupTable::parse(&table_bytes, &header, |offset| {
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| ParseError::Read)?;
        let mut bytes = [0; 16];
        file.read_exact(&mut bytes).map_err(|_| ParseError::Read)?;
        Ok(bytes)
    })?;
    let blob = table
        .blobs
        .iter()
        .find(|b| b.hash == hash)
        .ok_or(ParseError::ResourceNotFound)?;
    if start > end || end > blob.size {
        return Err(ParseError::InvalidParam.into());
    }
    let resource = &table.resources[blob.resource_index];
    let output = read_resource_range(
        &mut file,
        &resource.header,
        Compression::from_i32(resource.compression_code as i32)?,
        resource.chunk_size,
        if resource.solid {
            ResourceLayout::Solid
        } else {
            layout
        },
        blob.offset.checked_add(start).ok_or("overflow")?
            ..blob.offset.checked_add(end).ok_or("overflow")?,
        decode,
    )?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[5])?
        .write_all(&output)?;
    Ok(())
}
