//! Bounded decoding of Windows FILE_STREAM_INFORMATION records.
use wim_format::ParseError;

/// One logical NTFS data stream; an empty name denotes the unnamed stream.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DataStream {
    pub name: Vec<u16>,
    pub size: u64,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<DataStream>, ParseError> {
    let mut streams = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let record = bytes
            .get(offset..)
            .filter(|r| r.len() >= 24)
            .ok_or(ParseError::Read)?;
        let next =
            u32::from_le_bytes(record[..4].try_into().map_err(|_| ParseError::Read)?) as usize;
        let length =
            u32::from_le_bytes(record[4..8].try_into().map_err(|_| ParseError::Read)?) as usize;
        let size = i64::from_le_bytes(record[8..16].try_into().map_err(|_| ParseError::Read)?);
        let end = 24usize.checked_add(length).ok_or(ParseError::Read)?;
        if !length.is_multiple_of(2)
            || size < 0
            || (next != 0 && (next < end || !next.is_multiple_of(8) || next >= record.len()))
        {
            return Err(ParseError::Read);
        }
        let raw = record.get(24..end).ok_or(ParseError::Read)?;
        let name: Vec<u16> = raw
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        if name.first() != Some(&58) || name.contains(&0) {
            return Err(ParseError::Read);
        }
        let separator = name[1..]
            .iter()
            .position(|&u| u == 58)
            .map(|i| i + 1)
            .ok_or(ParseError::Read)?;
        if name[separator + 1..] == [36, 68, 65, 84, 65] {
            let name = &name[1..separator];
            if name.contains(&92)
                || name.contains(&47)
                || streams.iter().any(|s: &DataStream| s.name == name)
            {
                return Err(ParseError::Read);
            }
            // WIM stream counts have a u16 representation.
            if streams.len() == u16::MAX as usize {
                return Err(ParseError::Unsupported);
            }
            streams.try_reserve(1).map_err(|_| ParseError::Nomem)?;
            streams.push(DataStream {
                name: name.to_vec(),
                size: size as u64,
            });
        }
        if next == 0 {
            break;
        }
        offset = offset.checked_add(next).ok_or(ParseError::Read)?;
    }
    Ok(streams)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(name: &[u16], size: i64) -> Vec<u8> {
        let mut bytes = vec![0; 24];
        bytes[4..8].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
        bytes[8..16].copy_from_slice(&size.to_le_bytes());
        bytes.extend(name.iter().flat_map(|u| u.to_le_bytes()));
        bytes
    }
    #[test]
    fn decodes_empty_unicode_and_unnamed_streams_without_utf16_loss() {
        let mut name: Vec<u16> = ":stream-".encode_utf16().collect();
        name.push(0xd800);
        name.extend(":$DATA".encode_utf16());
        let stream = parse(&record(&name, 0)).unwrap().remove(0);
        assert_eq!(stream.name, &name[1..name.len() - 6]);
        assert_eq!(stream.size, 0);
        let unnamed: Vec<_> = "::$DATA".encode_utf16().collect();
        assert_eq!(
            parse(&record(&unnamed, 17)).unwrap(),
            [DataStream {
                name: vec![],
                size: 17
            }]
        );
    }
    #[test]
    fn rejects_truncation_negative_sizes_and_overlapping_or_unaligned_records() {
        let name: Vec<_> = ":ads:$DATA".encode_utf16().collect();
        let bytes = record(&name, 5);
        for end in 1..bytes.len() {
            assert!(parse(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        assert!(parse(&record(&name, -1)).is_err());
        for next in [8u32, 25, bytes.len() as u32, u32::MAX] {
            let mut invalid = bytes.clone();
            invalid[..4].copy_from_slice(&next.to_le_bytes());
            assert!(parse(&invalid).is_err());
        }
    }
    #[test]
    fn decodes_chains_and_rejects_duplicate_data_stream_names() {
        let name: Vec<_> = ":ads:$DATA".encode_utf16().collect();
        let mut first = record(&name, 5);
        first.resize(first.len().div_ceil(8) * 8, 0);
        let next = first.len() as u32;
        first[..4].copy_from_slice(&next.to_le_bytes());
        let mut duplicate = first.clone();
        duplicate.extend(record(&name, 5));
        assert!(parse(&duplicate).is_err());
        first.extend(record(&"::$DATA".encode_utf16().collect::<Vec<_>>(), 8));
        assert_eq!(parse(&first).unwrap().len(), 2);
    }
}
