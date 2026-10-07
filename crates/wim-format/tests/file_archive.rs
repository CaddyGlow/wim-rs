#![cfg(feature = "std")]
use std::{
    cell::Cell,
    io::{Cursor, Read, Seek, SeekFrom},
    rc::Rc,
};
use wim_format::{
    HEADER_SIZE, ParseError, archive::Archive, file_archive::FileArchive, integrity::IntegrityTable,
};
struct Counted {
    data: Cursor<&'static [u8]>,
    read: Rc<Cell<usize>>,
    virtual_len: u64,
}
impl Read for Counted {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.data.read(bytes)?;
        self.read.set(self.read.get() + n);
        Ok(n)
    }
}
impl Seek for Counted {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        match from {
            SeekFrom::End(delta) => self.data.seek(SeekFrom::Start(
                (i128::from(self.virtual_len) + i128::from(delta))
                    .try_into()
                    .map_err(|_| std::io::ErrorKind::InvalidInput)?,
            )),
            from => self.data.seek(from),
        }
    }
}
#[test]
fn open_reads_descriptors_without_touching_enclosing_payload_or_sparse_tail() {
    const BYTES: &[u8] = include_bytes!("fixtures/xpress-resource.wim");
    let expected = Archive::open(BYTES).unwrap();
    let count = Rc::new(Cell::new(0));
    let reader = Counted {
        data: Cursor::new(BYTES),
        read: count.clone(),
        virtual_len: 8 * 1024 * 1024 * 1024,
    };
    let archive = FileArchive::open(reader).unwrap();
    assert!(
        count.get()
            <= HEADER_SIZE
                + expected.header.blob_table.size_in_wim as usize
                + expected.lookup.resources.len() * 16
    );
    assert_eq!(archive.header, expected.header);
    for blob in &expected.lookup.blobs {
        assert_eq!(
            archive.read_blob(&blob.hash).unwrap(),
            expected.read_blob(&blob.hash).unwrap()
        );
    }
}
#[test]
fn file_backed_ranges_match_memory_readers_for_ordinary_pipable_and_solid_resources() {
    for bytes in [
        include_bytes!("fixtures/xpress-resource.wim").as_slice(),
        include_bytes!("fixtures/pipable-resource.wim").as_slice(),
        include_bytes!("fixtures/solid-resource.wim").as_slice(),
    ] {
        let memory = Archive::open(bytes).unwrap();
        let file = FileArchive::open(Cursor::new(bytes)).unwrap();
        for blob in &memory.lookup.blobs {
            for range in [0..blob.size, 0..blob.size.min(3), blob.size / 2..blob.size] {
                assert_eq!(
                    file.read_blob_range(&blob.hash, range.clone()).unwrap(),
                    memory.read_blob_range(&blob.hash, range).unwrap()
                );
            }
        }
    }
}
#[test]
fn streamed_integrity_matches_buffered_digests_and_progress() {
    let bytes: Vec<u8> = (0..HEADER_SIZE + 150_123)
        .map(|i| (i % 251) as u8)
        .collect();
    let mut progress = Vec::new();
    let actual = wim_format::integrity::calculate_file(
        &mut Cursor::new(&bytes),
        bytes.len() as u64,
        4096,
        |n, b| {
            progress.push((n, b));
            Ok(())
        },
    )
    .unwrap();
    let mut expected_progress = Vec::new();
    let expected =
        IntegrityTable::calculate_with_reuse(&bytes, bytes.len() as u64, 4096, None, |n, b| {
            expected_progress.push((n, b));
            Ok(())
        })
        .unwrap();
    assert_eq!(actual.encode().unwrap(), expected.encode().unwrap());
    assert_eq!(progress, expected_progress);
}
#[test]
fn streamed_integrity_preserves_callback_cancellation() {
    let bytes = vec![0; HEADER_SIZE + 8192];
    let result = wim_format::integrity::calculate_file(
        &mut Cursor::new(&bytes),
        bytes.len() as u64,
        4096,
        |n, _| {
            if n == 1 {
                Err(ParseError::AbortedByProgress)
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result.unwrap_err(), ParseError::AbortedByProgress);
}

#[test]
fn raw_truncation_is_rejected_before_an_impossible_advertised_allocation() {
    let header = wim_format::ResourceHeader {
        size_in_wim: 1,
        flags: 0,
        offset_in_wim: 0,
        uncompressed_size: u64::MAX / 4,
    };
    let result = wim_format::file_resource::read_resource_range(
        &mut Cursor::new([0]),
        &header,
        wim_format::Compression::None,
        0,
        wim_format::resource::ResourceLayout::Ordinary,
        0..header.uncompressed_size,
        |_, _, _| unreachable!("raw resources do not invoke a decoder"),
    );
    assert!(matches!(
        result,
        Err(wim_format::file_resource::FileReadError::Format(
            ParseError::UnexpectedEndOfFile
        ))
    ));
}
