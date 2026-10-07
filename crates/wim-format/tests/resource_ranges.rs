use wim_format::{
    Compression, ParseError,
    archive::Archive,
    resource::{ResourceLayout, read_resource_range},
};

#[test]
fn ranges_cross_chunks_without_decoding_nonintersecting_data() {
    for input in [
        include_bytes!("fixtures/xpress-resource.wim").as_slice(),
        include_bytes!("fixtures/pipable-resource.wim").as_slice(),
        include_bytes!("fixtures/solid-resource.wim").as_slice(),
    ] {
        let archive = Archive::open(input).unwrap();
        let blob = &archive.lookup.blobs[0];
        let resource = &archive.lookup.resources[blob.resource_index];
        let expected = archive.read_blob(&blob.hash).unwrap();
        let layout = if resource.solid {
            ResourceLayout::Solid
        } else if archive.header.magic == wim_format::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        for selection in [0..1, 3..27, 32760..32777, 65530..65550, 76800..76810, 0..0] {
            let mut calls = 0;
            let output = read_resource_range(
                input,
                &resource.header,
                Compression::from_i32(resource.compression_code as i32).unwrap(),
                resource.chunk_size,
                layout,
                selection.clone(),
                |kind, source, target| {
                    assert_eq!(kind, Compression::Xpress);
                    calls += 1;
                    ms_compress::decompress_xpress(source, target)
                        .map_err(|_| ParseError::Decompression)
                },
            )
            .unwrap();
            assert_eq!(
                output,
                expected[selection.start as usize..selection.end as usize]
            );
            assert!(calls <= 2);
        }
    }
}

#[test]
fn raw_ranges_validate_bounds_and_never_call_codec() {
    let header = wim_format::ResourceHeader {
        size_in_wim: 8,
        flags: 0,
        offset_in_wim: 2,
        uncompressed_size: 8,
    };
    assert_eq!(
        read_resource_range(
            b"0123456789",
            &header,
            Compression::None,
            0,
            ResourceLayout::Ordinary,
            2..5,
            |_, _, _| panic!("raw range called decoder")
        )
        .unwrap(),
        b"456"
    );
    assert_eq!(
        read_resource_range(
            b"0123456789",
            &header,
            Compression::None,
            0,
            ResourceLayout::Ordinary,
            8..9,
            |_, _, _| unreachable!()
        ),
        Err(ParseError::InvalidParam)
    );
}
