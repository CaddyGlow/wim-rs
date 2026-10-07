use wim_format::integrity::{
    DEFAULT_CHUNK_SIZE, IntegrityStatus, IntegrityTable, check_wim_integrity,
};
use wim_format::{HEADER_SIZE, Header, ParseError};
const FIXTURE: &[u8] = include_bytes!("fixtures/integrity.wim");
fn no_decode(_: wim_format::Compression, _: &[u8], _: &mut [u8]) -> Result<(), ParseError> {
    panic!("uncompressed fixture must not request decompression")
}
#[test]
fn upstream_generated_integrity_table_verifies_and_recalculates_identically() {
    let header = Header::parse_seekable(FIXTURE).unwrap();
    assert_eq!(
        check_wim_integrity(FIXTURE, &header, no_decode),
        Ok(IntegrityStatus::Ok)
    );
    let end = header.blob_table.offset_in_wim + header.blob_table.size_in_wim;
    let calculated = IntegrityTable::calculate(FIXTURE, end, DEFAULT_CHUNK_SIZE)
        .unwrap()
        .encode()
        .unwrap();
    let start = header.integrity_table.offset_in_wim as usize;
    assert_eq!(
        calculated,
        &FIXTURE[start..start + header.integrity_table.uncompressed_size as usize]
    );
}
#[test]
fn covered_corruption_fails_but_header_and_xml_corruption_are_outside_coverage() {
    let header = Header::parse_seekable(FIXTURE).unwrap();
    for offset in [
        HEADER_SIZE,
        (header.blob_table.offset_in_wim + header.blob_table.size_in_wim - 1) as usize,
    ] {
        let mut file = FIXTURE.to_vec();
        file[offset] ^= 1;
        assert_eq!(
            check_wim_integrity(&file, &header, no_decode),
            Ok(IntegrityStatus::Mismatch)
        );
    }
    for offset in [0, header.xml_data.offset_in_wim as usize] {
        let mut file = FIXTURE.to_vec();
        file[offset] ^= 1;
        assert_eq!(
            check_wim_integrity(&file, &header, no_decode),
            Ok(IntegrityStatus::Ok)
        );
    }
}
#[test]
fn chunk_boundaries_empty_and_partial_final_chunks_roundtrip() {
    for chunk_size in [1, 3, 4096] {
        for len in [
            0,
            1,
            chunk_size as usize - 1,
            chunk_size as usize,
            chunk_size as usize + 1,
            chunk_size as usize * 2,
        ] {
            let file: Vec<u8> = (0..HEADER_SIZE + len).map(|i| (i % 251) as u8).collect();
            let table = IntegrityTable::calculate(&file, file.len() as u64, chunk_size).unwrap();
            assert_eq!(table.digests().len(), len.div_ceil(chunk_size as usize));
            let encoded = table.encode().unwrap();
            assert_eq!(IntegrityTable::parse(&encoded, len as u64).unwrap(), table);
            assert_eq!(table.verify(&file, len as u64), Ok(true));
        }
    }
}
#[test]
fn malformed_table_fields_are_rejected() {
    let file = vec![42; HEADER_SIZE + 8];
    let bytes = IntegrityTable::calculate(&file, file.len() as u64, 4)
        .unwrap()
        .encode()
        .unwrap();
    for (offset, value) in [(0, 12u32), (4, 1), (8, 0), (8, 3)] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            IntegrityTable::parse(&bad, 8),
            Err(ParseError::InvalidIntegrityTable)
        );
    }
    for len in 0..bytes.len() {
        assert_eq!(
            IntegrityTable::parse(&bytes[..len], 8),
            Err(ParseError::InvalidIntegrityTable)
        );
    }
}
#[test]
fn truncation_is_distinct_from_mismatch_and_initial_cancellation_wins() {
    let file = vec![42; HEADER_SIZE + 8];
    let table = IntegrityTable::calculate(&file, file.len() as u64, 4).unwrap();
    assert_eq!(
        table.verify(&file[..HEADER_SIZE + 7], 8),
        Err(ParseError::UnexpectedEndOfFile)
    );
    let mut truncated = file[..HEADER_SIZE + 7].to_vec();
    truncated[HEADER_SIZE] ^= 1;
    assert_eq!(table.verify(&truncated, 8), Ok(false));
    assert_eq!(
        table.verify_with_progress(&[], 8, |_| Err(ParseError::AbortedByProgress)),
        Err(ParseError::AbortedByProgress)
    );
}
#[test]
fn progress_reports_initial_and_completed_chunks_and_cancels_after_chunk() {
    let file = vec![42; HEADER_SIZE + 7];
    let table = IntegrityTable::calculate(&file, file.len() as u64, 4).unwrap();
    let mut updates = Vec::new();
    assert_eq!(
        table.verify_with_progress(&file, 7, |p| {
            updates.push(p);
            Ok(())
        }),
        Ok(true)
    );
    assert_eq!(
        updates
            .iter()
            .map(|p| (p.completed_bytes, p.completed_chunks))
            .collect::<Vec<_>>(),
        [(0, 0), (4, 1), (7, 2)]
    );
    assert!(
        updates
            .iter()
            .all(|p| p.total_bytes == 7 && p.total_chunks == 2 && p.chunk_size == 4)
    );
    assert_eq!(
        table.verify_with_progress(&file, 7, |p| if p.completed_chunks == 1 {
            Err(ParseError::AbortedByProgress)
        } else {
            Ok(())
        }),
        Err(ParseError::AbortedByProgress)
    );
}
#[test]
fn absence_uses_offset_and_bad_coverage_is_rejected() {
    let mut header = Header::parse_seekable(FIXTURE).unwrap();
    header.integrity_table.offset_in_wim = 0;
    assert_eq!(
        check_wim_integrity(FIXTURE, &header, no_decode),
        Ok(IntegrityStatus::Nonexistent)
    );
    header = Header::parse_seekable(FIXTURE).unwrap();
    header.integrity_table.uncompressed_size = 0;
    assert_eq!(
        check_wim_integrity(FIXTURE, &header, no_decode),
        Err(ParseError::InvalidIntegrityTable)
    );
    header = Header::parse_seekable(FIXTURE).unwrap();
    header.blob_table.offset_in_wim = 0;
    header.blob_table.size_in_wim = 1;
    assert_eq!(
        check_wim_integrity(FIXTURE, &header, no_decode),
        Err(ParseError::InvalidIntegrityTable)
    );
    header.blob_table.offset_in_wim = u64::MAX;
    header.blob_table.size_in_wim = 1;
    assert_eq!(
        check_wim_integrity(FIXTURE, &header, no_decode),
        Err(ParseError::InvalidIntegrityTable)
    );
}

#[test]
fn upstream_accepts_nonstandard_chunk_size_and_partial_fourth_chunk() {
    let file = include_bytes!("fixtures/integrity-small-chunks.wim");
    let header = Header::parse_seekable(file).unwrap();
    assert_eq!(
        check_wim_integrity(file, &header, no_decode),
        Ok(IntegrityStatus::Ok)
    );
    let end = header.blob_table.offset_in_wim + header.blob_table.size_in_wim;
    let table = IntegrityTable::calculate(file, end, 128).unwrap();
    assert_eq!(table.digests().len(), 4);
    let start = header.integrity_table.offset_in_wim as usize;
    assert_eq!(table.encode().unwrap(), &file[start..]);
}

#[test]
fn archive_integrity_progress_preserves_absence_and_callback_failure_precedence() {
    use wim_format::integrity::check_wim_integrity_with_progress;
    let header = Header::parse_seekable(FIXTURE).unwrap();
    let mut events = Vec::new();
    assert_eq!(
        check_wim_integrity_with_progress(FIXTURE, &header, no_decode, |p| {
            events.push(p);
            Ok(())
        }),
        Ok(IntegrityStatus::Ok)
    );
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].completed_chunks, 0);
    assert_eq!(events[1].completed_bytes, events[1].total_bytes);
    let mut absent = header.clone();
    absent.integrity_table.offset_in_wim = 0;
    assert_eq!(
        check_wim_integrity_with_progress(FIXTURE, &absent, no_decode, |_| {
            panic!("absent tables must not emit progress")
        }),
        Ok(IntegrityStatus::Nonexistent)
    );
    let mut corrupted = FIXTURE.to_vec();
    corrupted[HEADER_SIZE] ^= 1;
    assert_eq!(
        check_wim_integrity_with_progress(&corrupted, &header, no_decode, |_| {
            Err(ParseError::AbortedByProgress)
        }),
        Err(ParseError::AbortedByProgress)
    );
}
