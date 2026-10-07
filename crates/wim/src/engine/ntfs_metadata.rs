//! Checked conversion between WIM packed EAs and NT FILE_FULL_EA_INFORMATION.
use wim_format::ParseError;

#[cfg(any(windows, test))]
pub(crate) fn unpack_eas(packed: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut result = Vec::new();
    let mut remaining = packed;
    let mut previous = None;
    while !remaining.is_empty() {
        if remaining.len() < 4 {
            return Err(ParseError::InvalidXattr);
        }
        let value = u16::from_le_bytes([remaining[0], remaining[1]]) as usize;
        let name = remaining[2] as usize;
        let size = 5 + name + value;
        if name == 0
            || remaining.len() < size
            || remaining[4..4 + name].contains(&0)
            || remaining[4 + name] != 0
            || remaining[3] & !0x80 != 0
        {
            return Err(ParseError::InvalidXattr);
        }
        let start = result.len();
        let length = (8 + name + 1 + value + 3) & !3;
        result.try_reserve(length).map_err(|_| ParseError::Nomem)?;
        result.resize(start + length, 0);
        if let Some(previous) = previous {
            result[previous..previous + 4]
                .copy_from_slice(&((start - previous) as u32).to_le_bytes());
        }
        result[start + 4] = remaining[3];
        result[start + 5] = remaining[2];
        result[start + 6..start + 8].copy_from_slice(&remaining[..2]);
        result[start + 8..start + 8 + name + 1 + value].copy_from_slice(&remaining[4..size]);
        previous = Some(start);
        remaining = &remaining[size..];
    }
    Ok(result)
}

#[cfg(any(windows, test))]
pub(crate) fn pack_eas(native: &[u8]) -> Result<Vec<u8>, ParseError> {
    pack_eas_with_policy(native, false).map(|(packed, _)| packed)
}

/// Microsoft portable capture excludes kernel-managed EAs, while the native manifest retains them.
pub(crate) fn pack_portable_eas(native: &[u8]) -> Result<(Vec<u8>, usize), ParseError> {
    pack_eas_with_policy(native, true)
}

fn pack_eas_with_policy(native: &[u8], portable: bool) -> Result<(Vec<u8>, usize), ParseError> {
    let mut packed = Vec::new();
    let mut omitted = 0;
    let mut remaining = native;
    while !remaining.is_empty() {
        if remaining.len() < 8 {
            return Err(ParseError::InvalidXattr);
        }
        let next = u32::from_le_bytes(
            remaining[..4]
                .try_into()
                .map_err(|_| ParseError::InvalidXattr)?,
        ) as usize;
        let name = remaining[5] as usize;
        let value = u16::from_le_bytes([remaining[6], remaining[7]]) as usize;
        let size = 9 + name + value;
        if name == 0
            || remaining.len() < size
            || remaining[8..8 + name].contains(&0)
            || remaining[8 + name] != 0
            || remaining[4] & !0x80 != 0
        {
            return Err(ParseError::InvalidXattr);
        }
        let kernel = remaining[8..8 + name]
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"$KERNEL."));
        if portable && kernel {
            omitted += 1;
        } else {
            packed
                .try_reserve(size - 4)
                .map_err(|_| ParseError::Nomem)?;
            packed.extend_from_slice(&remaining[6..8]);
            packed.extend_from_slice(&[remaining[5], remaining[4]]);
            packed.extend_from_slice(&remaining[8..size]);
        }
        if next == 0 {
            break;
        }
        if next < size || next > remaining.len() || !next.is_multiple_of(4) {
            return Err(ParseError::InvalidXattr);
        }
        remaining = &remaining[next..];
        if remaining.is_empty() {
            return Err(ParseError::InvalidXattr);
        }
    }
    Ok((packed, omitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_and_need_ea_records_round_trip() {
        let packed = b"\x04\x00\x03\x80ONE\0\0\xff\x12\0\x00\x00\x03\0TWO\0";
        assert_eq!(pack_eas(&unpack_eas(packed).unwrap()).unwrap(), packed);
    }
    #[test]
    fn portable_policy_omits_only_kernel_namespace_and_validates_omitted_records() {
        let mut packed = Vec::new();
        for name in [
            b"$Kernel.PURGE.ESBCACHE".as_slice(),
            b"$CI.CATALOGHINT",
            b"$KERNELX.VALUE",
        ] {
            packed.extend_from_slice(&[1, 0, name.len() as u8, 0]);
            packed.extend_from_slice(name);
            packed.extend_from_slice(&[0, 42]);
        }
        let native = unpack_eas(&packed).unwrap();
        let (portable, omitted) = pack_portable_eas(&native).unwrap();
        assert_eq!(omitted, 1);
        let retained = unpack_eas(&portable).unwrap();
        assert_eq!(pack_eas(&retained).unwrap(), portable);
        assert!(
            !portable
                .windows(8)
                .any(|name| name.eq_ignore_ascii_case(b"$KERNEL."))
        );
        assert!(
            portable
                .windows(b"$CI.CATALOGHINT\0".len())
                .any(|name| name == b"$CI.CATALOGHINT\0")
        );
        assert!(
            portable
                .windows(b"$KERNELX.VALUE".len())
                .any(|name| name == b"$KERNELX.VALUE")
        );
        let mut malformed = native;
        malformed[4] = 1;
        assert_eq!(
            pack_portable_eas(&malformed).unwrap_err(),
            ParseError::InvalidXattr
        );
    }

    #[test]
    fn malformed_ea_names_lengths_and_offsets_are_rejected() {
        for packed in [
            &b"\0"[..],
            &b"\0\0\0\0\0"[..],
            &b"\0\0\x02\0A\0\0"[..],
            &b"\0\0\x01\x01A\0"[..],
        ] {
            assert_eq!(unpack_eas(packed).unwrap_err(), ParseError::InvalidXattr);
        }
        let mut native = unpack_eas(b"\0\0\x01\0A\0").unwrap();
        native[..4].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(pack_eas(&native).unwrap_err(), ParseError::InvalidXattr);
    }
}
