// SPDX-License-Identifier: LGPL-2.1-or-later OR GPL-3.0-or-later
// Error and compression discriminants translated from wimlib include/wimlib.h.
//! Lossless native names and upstream-compatible numeric WIM constants.
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

/// Error codes assigned by the upstream wimlib public header.
/// Numeric gaps are intentional; unknown codes are rejected rather than cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ErrorCode {
    /// Upstream `WIMLIB_ERR_SUCCESS`.
    Success = 0,
    /// Upstream `WIMLIB_ERR_ALREADY_LOCKED`.
    AlreadyLocked = 1,
    /// Upstream `WIMLIB_ERR_DECOMPRESSION`.
    Decompression = 2,
    /// Upstream `WIMLIB_ERR_FUSE`.
    Fuse = 6,
    /// Upstream `WIMLIB_ERR_GLOB_HAD_NO_MATCHES`.
    GlobHadNoMatches = 8,
    /// Upstream `WIMLIB_ERR_IMAGE_COUNT`.
    ImageCount = 10,
    /// Upstream `WIMLIB_ERR_IMAGE_NAME_COLLISION`.
    ImageNameCollision = 11,
    /// Upstream `WIMLIB_ERR_INSUFFICIENT_PRIVILEGES`.
    InsufficientPrivileges = 12,
    /// Upstream `WIMLIB_ERR_INTEGRITY`.
    Integrity = 13,
    /// Upstream `WIMLIB_ERR_INVALID_CAPTURE_CONFIG`.
    InvalidCaptureConfig = 14,
    /// Upstream `WIMLIB_ERR_INVALID_CHUNK_SIZE`.
    InvalidChunkSize = 15,
    /// Upstream `WIMLIB_ERR_INVALID_COMPRESSION_TYPE`.
    InvalidCompressionType = 16,
    /// Upstream `WIMLIB_ERR_INVALID_HEADER`.
    InvalidHeader = 17,
    /// Upstream `WIMLIB_ERR_INVALID_IMAGE`.
    InvalidImage = 18,
    /// Upstream `WIMLIB_ERR_INVALID_INTEGRITY_TABLE`.
    InvalidIntegrityTable = 19,
    /// Upstream `WIMLIB_ERR_INVALID_LOOKUP_TABLE_ENTRY`.
    InvalidLookupTableEntry = 20,
    /// Upstream `WIMLIB_ERR_INVALID_METADATA_RESOURCE`.
    InvalidMetadataResource = 21,
    /// Upstream `WIMLIB_ERR_INVALID_OVERLAY`.
    InvalidOverlay = 23,
    /// Upstream `WIMLIB_ERR_INVALID_PARAM`.
    InvalidParam = 24,
    /// Upstream `WIMLIB_ERR_INVALID_PART_NUMBER`.
    InvalidPartNumber = 25,
    /// Upstream `WIMLIB_ERR_INVALID_PIPABLE_WIM`.
    InvalidPipableWim = 26,
    /// Upstream `WIMLIB_ERR_INVALID_REPARSE_DATA`.
    InvalidReparseData = 27,
    /// Upstream `WIMLIB_ERR_INVALID_RESOURCE_HASH`.
    InvalidResourceHash = 28,
    /// Upstream `WIMLIB_ERR_INVALID_UTF16_STRING`.
    InvalidUtf16String = 30,
    /// Upstream `WIMLIB_ERR_INVALID_UTF8_STRING`.
    InvalidUtf8String = 31,
    /// Upstream `WIMLIB_ERR_IS_DIRECTORY`.
    IsDirectory = 32,
    /// Upstream `WIMLIB_ERR_IS_SPLIT_WIM`.
    IsSplitWim = 33,
    /// Upstream `WIMLIB_ERR_LINK`.
    Link = 35,
    /// Upstream `WIMLIB_ERR_METADATA_NOT_FOUND`.
    MetadataNotFound = 36,
    /// Upstream `WIMLIB_ERR_MKDIR`.
    Mkdir = 37,
    /// Upstream `WIMLIB_ERR_MQUEUE`.
    Mqueue = 38,
    /// Upstream `WIMLIB_ERR_NOMEM`.
    Nomem = 39,
    /// Upstream `WIMLIB_ERR_NOTDIR`.
    Notdir = 40,
    /// Upstream `WIMLIB_ERR_NOTEMPTY`.
    Notempty = 41,
    /// Upstream `WIMLIB_ERR_NOT_A_REGULAR_FILE`.
    NotARegularFile = 42,
    /// Upstream `WIMLIB_ERR_NOT_A_WIM_FILE`.
    NotAWimFile = 43,
    /// Upstream `WIMLIB_ERR_NOT_PIPABLE`.
    NotPipable = 44,
    /// Upstream `WIMLIB_ERR_NO_FILENAME`.
    NoFilename = 45,
    /// Upstream `WIMLIB_ERR_NTFS_3G`.
    Ntfs3G = 46,
    /// Upstream `WIMLIB_ERR_OPEN`.
    Open = 47,
    /// Upstream `WIMLIB_ERR_OPENDIR`.
    Opendir = 48,
    /// Upstream `WIMLIB_ERR_PATH_DOES_NOT_EXIST`.
    PathDoesNotExist = 49,
    /// Upstream `WIMLIB_ERR_READ`.
    Read = 50,
    /// Upstream `WIMLIB_ERR_READLINK`.
    Readlink = 51,
    /// Upstream `WIMLIB_ERR_RENAME`.
    Rename = 52,
    /// Upstream `WIMLIB_ERR_REPARSE_POINT_FIXUP_FAILED`.
    ReparsePointFixupFailed = 54,
    /// Upstream `WIMLIB_ERR_RESOURCE_NOT_FOUND`.
    ResourceNotFound = 55,
    /// Upstream `WIMLIB_ERR_RESOURCE_ORDER`.
    ResourceOrder = 56,
    /// Upstream `WIMLIB_ERR_SET_ATTRIBUTES`.
    SetAttributes = 57,
    /// Upstream `WIMLIB_ERR_SET_REPARSE_DATA`.
    SetReparseData = 58,
    /// Upstream `WIMLIB_ERR_SET_SECURITY`.
    SetSecurity = 59,
    /// Upstream `WIMLIB_ERR_SET_SHORT_NAME`.
    SetShortName = 60,
    /// Upstream `WIMLIB_ERR_SET_TIMESTAMPS`.
    SetTimestamps = 61,
    /// Upstream `WIMLIB_ERR_SPLIT_INVALID`.
    SplitInvalid = 62,
    /// Upstream `WIMLIB_ERR_STAT`.
    Stat = 63,
    /// Upstream `WIMLIB_ERR_UNEXPECTED_END_OF_FILE`.
    UnexpectedEndOfFile = 65,
    /// Upstream `WIMLIB_ERR_UNICODE_STRING_NOT_REPRESENTABLE`.
    UnicodeStringNotRepresentable = 66,
    /// Upstream `WIMLIB_ERR_UNKNOWN_VERSION`.
    UnknownVersion = 67,
    /// Upstream `WIMLIB_ERR_UNSUPPORTED`.
    Unsupported = 68,
    /// Upstream `WIMLIB_ERR_UNSUPPORTED_FILE`.
    UnsupportedFile = 69,
    /// Upstream `WIMLIB_ERR_WIM_IS_READONLY`.
    WimIsReadonly = 71,
    /// Upstream `WIMLIB_ERR_WRITE`.
    Write = 72,
    /// Upstream `WIMLIB_ERR_XML`.
    Xml = 73,
    /// Upstream `WIMLIB_ERR_WIM_IS_ENCRYPTED`.
    WimIsEncrypted = 74,
    /// Upstream `WIMLIB_ERR_WIMBOOT`.
    Wimboot = 75,
    /// Upstream `WIMLIB_ERR_ABORTED_BY_PROGRESS`.
    AbortedByProgress = 76,
    /// Upstream `WIMLIB_ERR_UNKNOWN_PROGRESS_STATUS`.
    UnknownProgressStatus = 77,
    /// Upstream `WIMLIB_ERR_MKNOD`.
    Mknod = 78,
    /// Upstream `WIMLIB_ERR_MOUNTED_IMAGE_IS_BUSY`.
    MountedImageIsBusy = 79,
    /// Upstream `WIMLIB_ERR_NOT_A_MOUNTPOINT`.
    NotAMountpoint = 80,
    /// Upstream `WIMLIB_ERR_NOT_PERMITTED_TO_UNMOUNT`.
    NotPermittedToUnmount = 81,
    /// Upstream `WIMLIB_ERR_FVE_LOCKED_VOLUME`.
    FveLockedVolume = 82,
    /// Upstream `WIMLIB_ERR_UNABLE_TO_READ_CAPTURE_CONFIG`.
    UnableToReadCaptureConfig = 83,
    /// Upstream `WIMLIB_ERR_WIM_IS_INCOMPLETE`.
    WimIsIncomplete = 84,
    /// Upstream `WIMLIB_ERR_COMPACTION_NOT_POSSIBLE`.
    CompactionNotPossible = 85,
    /// Upstream `WIMLIB_ERR_IMAGE_HAS_MULTIPLE_REFERENCES`.
    ImageHasMultipleReferences = 86,
    /// Upstream `WIMLIB_ERR_DUPLICATE_EXPORTED_IMAGE`.
    DuplicateExportedImage = 87,
    /// Upstream `WIMLIB_ERR_CONCURRENT_MODIFICATION_DETECTED`.
    ConcurrentModificationDetected = 88,
    /// Upstream `WIMLIB_ERR_SNAPSHOT_FAILURE`.
    SnapshotFailure = 89,
    /// Upstream `WIMLIB_ERR_INVALID_XATTR`.
    InvalidXattr = 90,
    /// Upstream `WIMLIB_ERR_SET_XATTR`.
    SetXattr = 91,
}
impl ErrorCode {
    /// Returns the exact upstream error number.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
    /// Converts a known upstream code; reserved or unknown numbers return `None`.
    pub const fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Success),
            1 => Some(Self::AlreadyLocked),
            2 => Some(Self::Decompression),
            6 => Some(Self::Fuse),
            8 => Some(Self::GlobHadNoMatches),
            10 => Some(Self::ImageCount),
            11 => Some(Self::ImageNameCollision),
            12 => Some(Self::InsufficientPrivileges),
            13 => Some(Self::Integrity),
            14 => Some(Self::InvalidCaptureConfig),
            15 => Some(Self::InvalidChunkSize),
            16 => Some(Self::InvalidCompressionType),
            17 => Some(Self::InvalidHeader),
            18 => Some(Self::InvalidImage),
            19 => Some(Self::InvalidIntegrityTable),
            20 => Some(Self::InvalidLookupTableEntry),
            21 => Some(Self::InvalidMetadataResource),
            23 => Some(Self::InvalidOverlay),
            24 => Some(Self::InvalidParam),
            25 => Some(Self::InvalidPartNumber),
            26 => Some(Self::InvalidPipableWim),
            27 => Some(Self::InvalidReparseData),
            28 => Some(Self::InvalidResourceHash),
            30 => Some(Self::InvalidUtf16String),
            31 => Some(Self::InvalidUtf8String),
            32 => Some(Self::IsDirectory),
            33 => Some(Self::IsSplitWim),
            35 => Some(Self::Link),
            36 => Some(Self::MetadataNotFound),
            37 => Some(Self::Mkdir),
            38 => Some(Self::Mqueue),
            39 => Some(Self::Nomem),
            40 => Some(Self::Notdir),
            41 => Some(Self::Notempty),
            42 => Some(Self::NotARegularFile),
            43 => Some(Self::NotAWimFile),
            44 => Some(Self::NotPipable),
            45 => Some(Self::NoFilename),
            46 => Some(Self::Ntfs3G),
            47 => Some(Self::Open),
            48 => Some(Self::Opendir),
            49 => Some(Self::PathDoesNotExist),
            50 => Some(Self::Read),
            51 => Some(Self::Readlink),
            52 => Some(Self::Rename),
            54 => Some(Self::ReparsePointFixupFailed),
            55 => Some(Self::ResourceNotFound),
            56 => Some(Self::ResourceOrder),
            57 => Some(Self::SetAttributes),
            58 => Some(Self::SetReparseData),
            59 => Some(Self::SetSecurity),
            60 => Some(Self::SetShortName),
            61 => Some(Self::SetTimestamps),
            62 => Some(Self::SplitInvalid),
            63 => Some(Self::Stat),
            65 => Some(Self::UnexpectedEndOfFile),
            66 => Some(Self::UnicodeStringNotRepresentable),
            67 => Some(Self::UnknownVersion),
            68 => Some(Self::Unsupported),
            69 => Some(Self::UnsupportedFile),
            71 => Some(Self::WimIsReadonly),
            72 => Some(Self::Write),
            73 => Some(Self::Xml),
            74 => Some(Self::WimIsEncrypted),
            75 => Some(Self::Wimboot),
            76 => Some(Self::AbortedByProgress),
            77 => Some(Self::UnknownProgressStatus),
            78 => Some(Self::Mknod),
            79 => Some(Self::MountedImageIsBusy),
            80 => Some(Self::NotAMountpoint),
            81 => Some(Self::NotPermittedToUnmount),
            82 => Some(Self::FveLockedVolume),
            83 => Some(Self::UnableToReadCaptureConfig),
            84 => Some(Self::WimIsIncomplete),
            85 => Some(Self::CompactionNotPossible),
            86 => Some(Self::ImageHasMultipleReferences),
            87 => Some(Self::DuplicateExportedImage),
            88 => Some(Self::ConcurrentModificationDetected),
            89 => Some(Self::SnapshotFailure),
            90 => Some(Self::InvalidXattr),
            91 => Some(Self::SetXattr),
            _ => None,
        }
    }
}
impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?} (wimlib error {})", self.as_i32())
    }
}
impl core::error::Error for ErrorCode {}

/// Compression algorithms identified by the upstream public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum CompressionType {
    /// Uncompressed resources; invalid for constructing a compressor.
    None = 0,
    /// XPRESS Huffman compression.
    Xpress = 1,
    /// LZX compression.
    Lzx = 2,
    /// LZMS compression.
    Lzms = 3,
}
impl CompressionType {
    /// Returns the upstream algorithm number.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
    /// Decodes a public algorithm number.
    ///
    /// # Errors
    /// Returns `InvalidCompressionType` for unknown values.
    pub const fn from_i32(value: i32) -> Result<Self, ErrorCode> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Xpress),
            2 => Ok(Self::Lzx),
            3 => Ok(Self::Lzms),
            _ => Err(ErrorCode::InvalidCompressionType),
        }
    }
    /// Validates an on-disk WIM chunk size, including zero for uncompressed WIMs.
    /// This differs from raw compressor maximum block-size validation.
    ///
    /// # Errors
    /// Returns `InvalidChunkSize` outside the algorithm's power-of-two range.
    pub const fn validate_chunk_size(self, size: u32) -> Result<(), ErrorCode> {
        let (min, max) = match self {
            Self::None => (0, 0),
            Self::Xpress => (1 << 12, 1 << 16),
            Self::Lzx => (1 << 15, 1 << 21),
            Self::Lzms => (1 << 15, 1 << 30),
        };
        if (size == 0 || size.is_power_of_two()) && size >= min && size <= max {
            Ok(())
        } else {
            Err(ErrorCode::InvalidChunkSize)
        }
    }
}

/// An owned sequence of UTF-16 code units, preserving malformed surrogates.
///
/// This is a representation rather than a validated path component: NULs and
/// separators are retained. Path and metadata parsers apply their own rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WimName(Vec<u16>);
impl WimName {
    /// Takes ownership of code units without normalization or Unicode validation.
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }
    /// Borrows the exact stored code units.
    pub fn as_units(&self) -> &[u16] {
        &self.0
    }
    /// Returns the owned code units without copying.
    pub fn into_units(self) -> Vec<u16> {
        self.0
    }
    /// Converts valid UTF-16 to UTF-8 without replacement characters.
    ///
    /// # Errors
    /// Returns the standard UTF-16 error on an unpaired surrogate.
    pub fn to_string(&self) -> Result<String, alloc::string::FromUtf16Error> {
        String::from_utf16(&self.0)
    }
}
impl From<&str> for WimName {
    fn from(value: &str) -> Self {
        Self(value.encode_utf16().collect())
    }
}
