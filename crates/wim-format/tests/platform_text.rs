use wim_format::{
    ParseError,
    platform_text::{utf16_to_wtf8, wtf8_to_utf16},
};
#[test]
fn every_single_utf16_code_unit_round_trips_including_surrogates_and_noncharacters() {
    for unit in 0..=u16::MAX {
        let encoded = utf16_to_wtf8(&[unit]).unwrap();
        assert_eq!(wtf8_to_utf16(&encoded).unwrap(), [unit], "unit {unit:04x}");
    }
}
#[test]
fn paired_and_unpaired_surrogates_preserve_original_conversion_rules() {
    assert_eq!(utf16_to_wtf8(&[0xd800]).unwrap(), [0xed, 0xa0, 0x80]);
    assert_eq!(utf16_to_wtf8(&[0xdc00]).unwrap(), [0xed, 0xb0, 0x80]);
    assert_eq!(
        utf16_to_wtf8(&[0xd800, 0xdc00]).unwrap(),
        [0xf0, 0x90, 0x80, 0x80]
    );
    assert_eq!(
        wtf8_to_utf16(&[0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80]).unwrap(),
        [0xd800, 0xdc00]
    );
}
#[test]
fn malformed_utf8_bytes_fail_instead_of_being_replaced() {
    for invalid in [
        &b"\xff"[..],
        b"\x80",
        b"\xc0\x80",
        b"\xe0\x80\x80",
        b"\xed\xa0",
        b"\xf0\x80\x80\x80",
        b"\xf4\x90\x80\x80",
        b"\xf8\x80\x80\x80",
        b"\xe2\x28\xa1",
    ] {
        assert_eq!(wtf8_to_utf16(invalid), Err(ParseError::InvalidUtf8String));
    }
}
