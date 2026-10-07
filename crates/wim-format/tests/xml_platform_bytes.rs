use wim_format::{ParseError, xml::XmlInfo};

fn fixture() -> XmlInfo {
    XmlInfo::parse("<WIM><IMAGE INDEX=\"1\"><NAME>First</NAME></IMAGE><IMAGE INDEX=\"2\"><NAME>Second</NAME></IMAGE></WIM>").unwrap()
}
#[test]
fn raw_byte_names_participate_in_exact_collision_and_selector_rules() {
    let mut xml = fixture();
    xml.set_property_bytes(1, b"NAME", Some(b"\xff")).unwrap();
    assert_eq!(xml.name_bytes(1), Some(b"\xff".as_slice()));
    assert_eq!(xml.name(1), None);
    assert_eq!(xml.resolve_image_bytes(Some(b"\xff")), 1);
    assert!(xml.name_in_use_bytes(b"\xff"));
    assert_eq!(
        xml.set_property_bytes(2, b"NAME", Some(b"\xff")),
        Err(ParseError::ImageNameCollision)
    );
    // Distinct valid Unicode is never conflated with raw platform bytes.
    xml.set_name(2, Some("ÿ\u{e000}")).unwrap();
    assert_eq!(xml.resolve_image(Some("ÿ\u{e000}")), 2);
}
#[test]
fn non_utf8_element_names_and_values_remain_in_the_owned_tree_until_removed() {
    let mut xml = fixture();
    xml.set_property_bytes(1, b"\xfe/NODE[1]", Some(b"\xfd\xfc<&"))
        .unwrap();
    assert_eq!(
        xml.get_property_bytes(1, b"\xfe/NODE"),
        Some(b"\xfd\xfc<&".as_slice())
    );
    let expected = b"<NODE>\xfd\xfc&lt;&amp;";
    assert!(
        xml.to_xml_bytes()
            .windows(expected.len())
            .any(|w| w == expected)
    );
    assert_eq!(xml.encode_utf16le(), Err(ParseError::InvalidUtf8String));
    xml.set_property_bytes(1, b"\xfe/NODE", None).unwrap();
    assert_eq!(xml.encode_utf16le(), Err(ParseError::InvalidUtf8String));
    xml.set_property_bytes(1, b"\xfe", None).unwrap();
    let reopened = XmlInfo::parse_utf16le(&xml.encode_utf16le().unwrap()).unwrap();
    assert_eq!(reopened.name(1), Some("First"));
}
#[test]
fn image_selection_and_append_preserve_raw_platform_properties() {
    let mut xml = fixture();
    xml.set_property_bytes(1, b"NAME", Some(b"\xff")).unwrap();
    let selected = xml.select_images(&[1]).unwrap();
    assert_eq!(selected.name_bytes(1), Some(b"\xff".as_slice()));
    let mut destination = XmlInfo::parse("<WIM/>").unwrap();
    destination.append_images(&selected, &[1]).unwrap();
    assert_eq!(destination.name_bytes(1), Some(b"\xff".as_slice()));
    assert_eq!(
        destination.encode_utf16le(),
        Err(ParseError::InvalidUtf8String)
    );
    destination.set_name(1, Some("corrected")).unwrap();
    assert!(destination.encode_utf16le().is_ok());
}

#[test]
fn unpaired_surrogates_survive_utf16_xml_parsing_and_writing() {
    let mut xml = fixture();
    xml.set_property_bytes(1, b"NAME", Some(b"\xed\xa0\x80"))
        .unwrap();
    xml.set_property_bytes(1, b"CUSTOM/\xed\xb0\x80", Some(b"\xed\xbf\xbf<&"))
        .unwrap();
    assert_eq!(xml.to_xml(), Err(ParseError::InvalidUtf8String));
    let encoded = xml.encode_utf16le().unwrap();
    let reopened = XmlInfo::parse_utf16le(&encoded).unwrap();
    assert_eq!(reopened.name_bytes(1), Some(b"\xed\xa0\x80".as_slice()));
    assert_eq!(
        reopened.get_property_bytes(1, b"CUSTOM/\xed\xb0\x80"),
        Some(b"\xed\xbf\xbf<&".as_slice())
    );
    assert_eq!(reopened.encode_utf16le().unwrap(), encoded);
}
#[test]
fn separately_encoded_surrogate_pair_normalizes_after_utf16_resource_roundtrip() {
    let mut xml = fixture();
    xml.set_property_bytes(1, b"NAME", Some(b"\xed\xa0\x80\xed\xb0\x80"))
        .unwrap();
    let reopened = XmlInfo::parse_utf16le(&xml.encode_utf16le().unwrap()).unwrap();
    assert_eq!(reopened.name_bytes(1), Some(b"\xf0\x90\x80\x80".as_slice()));
}
