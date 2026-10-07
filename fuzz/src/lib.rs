use wim_format::{Header, archive::Archive, metadata::Metadata};

const ARCHIVE_LIMIT: usize = 1 << 20;
const BLOCK_LIMIT: usize = 32768;

/// Exercise WIM headers, raw metadata, lookup tables, XML and blob decoding.
pub fn wim(data: &[u8]) {
    if data.len() > ARCHIVE_LIMIT {
        return;
    }
    let _ = Metadata::parse(data);
    let Ok(header) = Header::parse_seekable(data) else {
        return;
    };
    assert_eq!(
        Header::parse(&header.encode(), Some(data.len() as u64)),
        Ok(header.clone())
    );
    // Mutated size fields must not turn a tiny input into a huge allocation.
    if header.chunk_size as usize > ARCHIVE_LIMIT
        || [header.blob_table, header.xml_data, header.integrity_table]
            .iter()
            .any(|r| r.uncompressed_size > ARCHIVE_LIMIT as u64)
    {
        return;
    }
    // Solid resources carry a second size/chunk header inside the file.
    for resource in [header.blob_table, header.xml_data, header.integrity_table] {
        if resource.flags & 16 != 0 {
            let Ok(start) = usize::try_from(resource.offset_in_wim) else {
                return;
            };
            let Some(end) = start.checked_add(16) else {
                return;
            };
            let Some(raw) = data.get(start..end) else {
                return;
            };
            let size = u64::from_le_bytes(raw[..8].try_into().expect("fixed size field"));
            let chunk = u32::from_le_bytes(raw[8..12].try_into().expect("fixed chunk field"));
            if size > ARCHIVE_LIMIT as u64 || chunk as usize > ARCHIVE_LIMIT {
                return;
            }
        }
    }
    let Ok(archive) = Archive::open(data) else {
        return;
    };
    let _ = archive.xml();
    let _ = archive.check_integrity();
    for blob in archive.lookup.blobs.iter().take(32) {
        // Read ranges rather than allocating the declared whole blob.
        if archive.lookup.resources[blob.resource_index].chunk_size as usize > ARCHIVE_LIMIT {
            continue;
        }
        let _ = archive.read_blob_range(&blob.hash, 0..blob.size.min(BLOCK_LIMIT as u64));
    }
    for (index, blob) in archive.lookup.metadata.iter().take(32).enumerate() {
        let resource = &archive.lookup.resources[blob.resource_index];
        if blob.size <= ARCHIVE_LIMIT as u64
            && resource.uncompressed_size <= ARCHIVE_LIMIT as u64
            && resource.chunk_size as usize <= ARCHIVE_LIMIT
            && let Ok(bytes) = archive.read_metadata(index as u32 + 1)
        {
            let _ = Metadata::parse(&bytes);
        }
    }
}
