use wim_format::{
    Compression, ParseError, ResourceHeader,
    resource::{ResourceLayout, read_resource},
};
fn decode(c: Compression, input: &[u8], out: &mut [u8]) -> Result<(), ParseError> {
    assert_eq!(c, Compression::Xpress);
    ms_compress::decompress_xpress(input, out).map_err(|_| ParseError::Decompression)
}
#[test]
fn upstream_xpress_resource_matches_original_payload() {
    let w = include_bytes!("fixtures/xpress-resource.wim");
    let r = ResourceHeader::parse(include_bytes!("fixtures/xpress-resource.descriptor")).unwrap();
    let got = read_resource(
        w,
        &r,
        Compression::Xpress,
        32768,
        ResourceLayout::Ordinary,
        decode,
    )
    .unwrap();
    let mut expected = (0..300).flat_map(|_| 0u8..=255).collect::<Vec<_>>();
    expected.extend_from_slice(b"last chunk");
    assert_eq!(got, expected);
}
#[test]
fn raw_chunks_and_partial_last_chunk_are_copied_without_decoder() {
    let r = ResourceHeader {
        size_in_wim: 11,
        flags: 4,
        offset_in_wim: 0,
        uncompressed_size: 7,
    };
    let bytes = [4, 0, 0, 0, b'a', b'b', b'c', b'd', b'e', b'f', b'g'];
    assert_eq!(
        read_resource(
            &bytes,
            &r,
            Compression::Xpress,
            4,
            ResourceLayout::Ordinary,
            |_, _, _| panic!("raw chunk")
        )
        .unwrap(),
        b"abcdefg"
    );
}
#[test]
fn malformed_ranges_and_chunk_offsets_return_errors() {
    let r = ResourceHeader {
        size_in_wim: 8,
        flags: 4,
        offset_in_wim: 0,
        uncompressed_size: 8,
    };
    assert_eq!(
        read_resource(
            &[0; 7],
            &r,
            Compression::Xpress,
            4,
            ResourceLayout::Ordinary,
            decode
        ),
        Err(ParseError::UnexpectedEndOfFile)
    );
    assert_eq!(
        read_resource(
            &[255; 8],
            &r,
            Compression::Xpress,
            4,
            ResourceLayout::Ordinary,
            decode
        ),
        Err(ParseError::Decompression)
    );
    let r = ResourceHeader {
        offset_in_wim: u64::MAX,
        ..r
    };
    assert_eq!(
        read_resource(
            &[],
            &r,
            Compression::Xpress,
            4,
            ResourceLayout::Ordinary,
            decode
        ),
        Err(ParseError::UnexpectedEndOfFile)
    );
}
#[test]
fn pipable_and_solid_raw_chunk_layouts_decode() {
    let r = ResourceHeader {
        size_in_wim: 19,
        flags: 4,
        offset_in_wim: 0,
        uncompressed_size: 7,
    };
    let mut p = vec![];
    p.extend(4u32.to_le_bytes());
    p.extend(b"abcd");
    p.extend(3u32.to_le_bytes());
    p.extend(b"efg");
    p.extend(4u32.to_le_bytes());
    assert_eq!(
        read_resource(
            &p,
            &r,
            Compression::Xpress,
            4,
            ResourceLayout::Pipable,
            decode
        )
        .unwrap(),
        b"abcdefg"
    );
    let mut s = vec![];
    s.extend(7u64.to_le_bytes());
    s.extend(4u32.to_le_bytes());
    s.extend(1u32.to_le_bytes());
    s.extend(4u32.to_le_bytes());
    s.extend(3u32.to_le_bytes());
    s.extend(b"abcdefg");
    let r = ResourceHeader {
        size_in_wim: 31,
        flags: 16,
        offset_in_wim: 0,
        uncompressed_size: 7,
    };
    assert_eq!(
        read_resource(&s, &r, Compression::Lzms, 0, ResourceLayout::Solid, decode).unwrap(),
        b"abcdefg"
    );
}
#[test]
fn resources_larger_than_32bits_use_64bit_offsets_before_allocating() {
    let mut b = vec![];
    b.extend(0x1_0000_0001u64.to_le_bytes());
    b.extend([0; 4]);
    let r = ResourceHeader {
        size_in_wim: 12,
        flags: 4,
        offset_in_wim: 0,
        uncompressed_size: 0x1_0000_0000,
    };
    assert_eq!(
        read_resource(
            &b,
            &r,
            Compression::Xpress,
            0x8000_0000,
            ResourceLayout::Ordinary,
            decode
        ),
        Err(ParseError::Decompression)
    );
}
#[test]
fn upstream_pipable_and_solid_xpress_resources_match_payload() {
    for (w, r, layout) in [
        (
            include_bytes!("fixtures/pipable-resource.wim").as_slice(),
            include_bytes!("fixtures/pipable-resource.descriptor").as_slice(),
            ResourceLayout::Pipable,
        ),
        (
            include_bytes!("fixtures/solid-resource.wim").as_slice(),
            include_bytes!("fixtures/solid-resource.descriptor").as_slice(),
            ResourceLayout::Solid,
        ),
    ] {
        let r = ResourceHeader::parse(r).unwrap();
        let actual = read_resource(w, &r, Compression::Xpress, 32768, layout, decode).unwrap();
        let mut expected = (0..300).flat_map(|_| 0u8..=255).collect::<Vec<_>>();
        expected.extend_from_slice(b"last chunk");
        assert_eq!(actual, expected);
    }
}
#[test]
fn uncompressed_reads_use_requested_uncompressed_size() {
    let r = ResourceHeader {
        size_in_wim: 999,
        flags: 2,
        offset_in_wim: 1,
        uncompressed_size: 3,
    };
    assert_eq!(
        read_resource(
            b"xabcd",
            &r,
            Compression::None,
            0,
            ResourceLayout::Ordinary,
            decode
        )
        .unwrap(),
        b"abc"
    );
}
#[test]
fn solid_last_chunk_table_entry_is_ignored_and_algorithm_is_22bit() {
    let original = include_bytes!("fixtures/solid-resource.wim");
    let r = ResourceHeader::parse(include_bytes!("fixtures/solid-resource.descriptor")).unwrap();
    for high_bits in [0, 1u32 << 22] {
        let mut w = original.to_vec();
        let start = r.offset_in_wim as usize;
        w[start + 12..start + 16].copy_from_slice(&(1u32 | high_bits).to_le_bytes());
        w[start + 24..start + 28].copy_from_slice(&0u32.to_le_bytes());
        let got = read_resource(
            &w,
            &r,
            Compression::Xpress,
            32768,
            ResourceLayout::Solid,
            decode,
        )
        .unwrap();
        let mut expected = (0..300).flat_map(|_| 0u8..=255).collect::<Vec<_>>();
        expected.extend_from_slice(b"last chunk");
        assert_eq!(got, expected);
    }
}
