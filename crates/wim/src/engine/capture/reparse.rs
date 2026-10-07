//! Original reparse link parsing and canonical rebuilding for capture fixups.
use wim_format::ParseError;
pub(crate) const SYMLINK: u32 = 0xa000000c;
pub(crate) const JUNCTION: u32 = 0xa0000003;
pub(crate) struct Link<'a> {
    pub tag: u32,
    pub reserved: u16,
    pub flags: u32,
    pub substitute: &'a [u8],
    pub print: &'a [u8],
}
/// Unrecognized/malformed link buffers are preserved without fixup, as upstream.
pub(crate) fn parse(bytes: &[u8]) -> Option<Link<'_>> {
    if bytes.len() < 16 {
        return None;
    }
    let word = |offset| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
    let tag = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    let (data, flags) = match tag {
        SYMLINK if bytes.len() >= 20 => (20, u32::from_le_bytes(bytes[16..20].try_into().unwrap())),
        JUNCTION => (16, 0),
        _ => return None,
    };
    let (sub_offset, sub_length, print_offset, print_length) =
        (word(8), word(10), word(12), word(14));
    if (sub_offset | sub_length | print_offset | print_length) & 1 != 0 {
        return None;
    }
    Some(Link {
        tag,
        flags,
        reserved: word(6) as u16,
        substitute: bytes.get(data + sub_offset..data + sub_offset + sub_length)?,
        print: bytes.get(data + print_offset..data + print_offset + print_length)?,
    })
}
pub(crate) fn make(
    link: &Link<'_>,
    substitute: &[u8],
    print: &[u8],
) -> Result<Vec<u8>, ParseError> {
    let base = match link.tag {
        SYMLINK => 20,
        JUNCTION => 16,
        _ => return Err(ParseError::InvalidReparseData),
    };
    let total = base + substitute.len() + print.len() + 4;
    if total > 16384 || (substitute.len() | print.len()) & 1 != 0 {
        return Err(ParseError::InvalidReparseData);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| ParseError::Nomem)?;
    out.extend_from_slice(&link.tag.to_le_bytes());
    out.extend_from_slice(&((total - 8) as u16).to_le_bytes());
    out.extend_from_slice(&link.reserved.to_le_bytes());
    for value in [
        0,
        substitute.len() as u16,
        (substitute.len() + 2) as u16,
        print.len() as u16,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    if link.tag == SYMLINK {
        out.extend_from_slice(&link.flags.to_le_bytes());
    }
    out.extend_from_slice(substitute);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(print);
    out.extend_from_slice(&[0, 0]);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_links_preserve_tag_reserved_flags_and_bounded_names() {
        let name: Vec<_> = "\\??\\X:\\target"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        for tag in [SYMLINK, JUNCTION] {
            let link = Link {
                tag,
                reserved: 17,
                flags: 1,
                substitute: &[],
                print: &[],
            };
            let bytes = make(&link, &name, &name[8..]).unwrap();
            let parsed = parse(&bytes).unwrap();
            assert_eq!(parsed.substitute, name);
            assert_eq!(parsed.print, &name[8..]);
            assert_eq!(parsed.reserved, 17);
            assert_eq!(parsed.flags, if tag == SYMLINK { 1 } else { 0 });
            let mut odd = bytes.clone();
            odd[8] = 1;
            assert!(parse(&odd).is_none());
            assert!(parse(&bytes[..15]).is_none());
        }
    }
}
