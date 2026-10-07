use wim_types::{CompressionType, ErrorCode, WimName};

#[test]
fn errors_preserve_sparse_upstream_numbers() {
    assert_eq!(ErrorCode::InvalidHeader.as_i32(), 17);
    assert_eq!(ErrorCode::SetXattr.as_i32(), 91);
    for value in [3, 4, 5, 7, 9, 22, 29, 34, 53, 64, 70, -1, 92] {
        assert_eq!(ErrorCode::from_i32(value), None);
    }
}

#[test]
fn compression_chunk_limits_are_format_specific() {
    for (kind, low, high) in [
        (CompressionType::Xpress, 12, 16),
        (CompressionType::Lzx, 15, 21),
        (CompressionType::Lzms, 15, 30),
    ] {
        for shift in low..=high {
            assert_eq!(kind.validate_chunk_size(1 << shift), Ok(()));
        }
        assert_eq!(
            kind.validate_chunk_size(1 << (low - 1)),
            Err(ErrorCode::InvalidChunkSize)
        );
        assert_eq!(
            kind.validate_chunk_size((1 << low) + 1),
            Err(ErrorCode::InvalidChunkSize)
        );
    }
    assert_eq!(
        CompressionType::from_i32(4),
        Err(ErrorCode::InvalidCompressionType)
    );
}

#[test]
fn native_names_preserve_unpaired_surrogates_and_nuls() {
    let units = vec![0xd800, 0, 0xdc00, 0x61];
    let name = WimName::from_units(units.clone());
    assert_eq!(name.as_units(), units);
    assert!(name.to_string().is_err());
    assert_eq!(name.into_units(), units);
}

#[test]
fn native_names_encode_supplementary_unicode_losslessly() {
    let name = WimName::from("A😀");
    assert_eq!(name.as_units(), &[0x41, 0xd83d, 0xde00]);
    assert_eq!(name.to_string().unwrap(), "A😀");
}

// Numeric contract transcribed from upstream include/wimlib.h 1.14.5.
#[test]
fn all_upstream_error_discriminants_roundtrip_without_filling_reserved_gaps() {
    let known = [
        (0, ErrorCode::Success),
        (1, ErrorCode::AlreadyLocked),
        (2, ErrorCode::Decompression),
        (6, ErrorCode::Fuse),
        (8, ErrorCode::GlobHadNoMatches),
        (10, ErrorCode::ImageCount),
        (11, ErrorCode::ImageNameCollision),
        (12, ErrorCode::InsufficientPrivileges),
        (13, ErrorCode::Integrity),
        (14, ErrorCode::InvalidCaptureConfig),
        (15, ErrorCode::InvalidChunkSize),
        (16, ErrorCode::InvalidCompressionType),
        (17, ErrorCode::InvalidHeader),
        (18, ErrorCode::InvalidImage),
        (19, ErrorCode::InvalidIntegrityTable),
        (20, ErrorCode::InvalidLookupTableEntry),
        (21, ErrorCode::InvalidMetadataResource),
        (23, ErrorCode::InvalidOverlay),
        (24, ErrorCode::InvalidParam),
        (25, ErrorCode::InvalidPartNumber),
        (26, ErrorCode::InvalidPipableWim),
        (27, ErrorCode::InvalidReparseData),
        (28, ErrorCode::InvalidResourceHash),
        (30, ErrorCode::InvalidUtf16String),
        (31, ErrorCode::InvalidUtf8String),
        (32, ErrorCode::IsDirectory),
        (33, ErrorCode::IsSplitWim),
        (35, ErrorCode::Link),
        (36, ErrorCode::MetadataNotFound),
        (37, ErrorCode::Mkdir),
        (38, ErrorCode::Mqueue),
        (39, ErrorCode::Nomem),
        (40, ErrorCode::Notdir),
        (41, ErrorCode::Notempty),
        (42, ErrorCode::NotARegularFile),
        (43, ErrorCode::NotAWimFile),
        (44, ErrorCode::NotPipable),
        (45, ErrorCode::NoFilename),
        (46, ErrorCode::Ntfs3G),
        (47, ErrorCode::Open),
        (48, ErrorCode::Opendir),
        (49, ErrorCode::PathDoesNotExist),
        (50, ErrorCode::Read),
        (51, ErrorCode::Readlink),
        (52, ErrorCode::Rename),
        (54, ErrorCode::ReparsePointFixupFailed),
        (55, ErrorCode::ResourceNotFound),
        (56, ErrorCode::ResourceOrder),
        (57, ErrorCode::SetAttributes),
        (58, ErrorCode::SetReparseData),
        (59, ErrorCode::SetSecurity),
        (60, ErrorCode::SetShortName),
        (61, ErrorCode::SetTimestamps),
        (62, ErrorCode::SplitInvalid),
        (63, ErrorCode::Stat),
        (65, ErrorCode::UnexpectedEndOfFile),
        (66, ErrorCode::UnicodeStringNotRepresentable),
        (67, ErrorCode::UnknownVersion),
        (68, ErrorCode::Unsupported),
        (69, ErrorCode::UnsupportedFile),
        (71, ErrorCode::WimIsReadonly),
        (72, ErrorCode::Write),
        (73, ErrorCode::Xml),
        (74, ErrorCode::WimIsEncrypted),
        (75, ErrorCode::Wimboot),
        (76, ErrorCode::AbortedByProgress),
        (77, ErrorCode::UnknownProgressStatus),
        (78, ErrorCode::Mknod),
        (79, ErrorCode::MountedImageIsBusy),
        (80, ErrorCode::NotAMountpoint),
        (81, ErrorCode::NotPermittedToUnmount),
        (82, ErrorCode::FveLockedVolume),
        (83, ErrorCode::UnableToReadCaptureConfig),
        (84, ErrorCode::WimIsIncomplete),
        (85, ErrorCode::CompactionNotPossible),
        (86, ErrorCode::ImageHasMultipleReferences),
        (87, ErrorCode::DuplicateExportedImage),
        (88, ErrorCode::ConcurrentModificationDetected),
        (89, ErrorCode::SnapshotFailure),
        (90, ErrorCode::InvalidXattr),
        (91, ErrorCode::SetXattr),
    ];
    for (value, code) in known {
        assert_eq!(code.as_i32(), value);
        assert_eq!(ErrorCode::from_i32(value), Some(code));
    }
    for value in -1..=92 {
        assert_eq!(
            ErrorCode::from_i32(value).is_some(),
            known.iter().any(|&(number, _)| number == value)
        );
    }
}

#[test]
fn uncompressed_chunks_require_zero_and_compressed_chunks_reject_zero() {
    assert_eq!(CompressionType::None.validate_chunk_size(0), Ok(()));
    assert_eq!(
        CompressionType::None.validate_chunk_size(32768),
        Err(ErrorCode::InvalidChunkSize)
    );
    for kind in [
        CompressionType::Xpress,
        CompressionType::Lzx,
        CompressionType::Lzms,
    ] {
        assert_eq!(
            kind.validate_chunk_size(0),
            Err(ErrorCode::InvalidChunkSize)
        );
    }
}

#[test]
fn compression_numbers_roundtrip_and_reject_negative_or_unknown_values() {
    for (number, kind) in [
        (0, CompressionType::None),
        (1, CompressionType::Xpress),
        (2, CompressionType::Lzx),
        (3, CompressionType::Lzms),
    ] {
        assert_eq!(kind.as_i32(), number);
        assert_eq!(CompressionType::from_i32(number), Ok(kind));
    }
    for number in [-1, 4, i32::MIN, i32::MAX] {
        assert_eq!(
            CompressionType::from_i32(number),
            Err(ErrorCode::InvalidCompressionType)
        );
    }
}
