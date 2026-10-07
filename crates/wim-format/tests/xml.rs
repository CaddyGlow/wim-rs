use wim_format::{ParseError, xml::XmlInfo};
#[test]
fn indexed_paths_create_only_next_sibling_and_delete_empty_values() {
    let mut x = XmlInfo::parse("<WIM><IMAGE INDEX='1'><NAME>A</NAME></IMAGE></WIM>").unwrap();
    assert_eq!(
        x.set_property(1, "WINDOWS/LANGUAGES/LANGUAGE[2]", Some("fr")),
        Err(ParseError::InvalidParam)
    );
    x.set_property(1, "WINDOWS/LANGUAGES/LANGUAGE", Some("en"))
        .unwrap();
    x.set_property(1, "WINDOWS/LANGUAGES/LANGUAGE[2]", Some("fr"))
        .unwrap();
    assert_eq!(
        x.get_property(1, "WINDOWS/LANGUAGES/LANGUAGE[2]"),
        Some("fr")
    );
    x.set_property(1, "WINDOWS/LANGUAGES/LANGUAGE[1]", Some(""))
        .unwrap();
    assert_eq!(x.get_property(1, "WINDOWS/LANGUAGES/LANGUAGE"), Some("fr"));
}
#[test]
fn names_collide_case_sensitively_and_validation_precedes_image_check() {
    let mut x = XmlInfo::parse(
        "<WIM><IMAGE INDEX='2'><NAME>B</NAME></IMAGE><IMAGE INDEX='1'><NAME>A</NAME></IMAGE></WIM>",
    )
    .unwrap();
    assert_eq!(x.name(1), Some("A"));
    assert_eq!(
        x.set_name(1, Some("B")),
        Err(ParseError::ImageNameCollision)
    );
    assert_eq!(
        x.set_property(0, "bad space", None),
        Err(ParseError::InvalidParam)
    );
    assert_eq!(x.set_name(0, None), Err(ParseError::InvalidImage));
    x.set_name(1, Some("b")).unwrap();
    x.set_name(1, None).unwrap();
    assert_eq!(x.name(1), Some(""));
}
#[test]
fn preserves_unknown_attributes_mixed_text_and_utf16_supplementary_characters() {
    let mut x = XmlInfo::parse("<?xml version='1.0'?><WIM vendor='x'><IMAGE INDEX='1'><CUSTOM a='&amp;'>one<![CDATA[😀]]><X/>two</CUSTOM></IMAGE></WIM>").unwrap();
    assert_eq!(x.get_property(1, "CUSTOM"), Some("one😀"));
    x.set_description(1, Some("雪 & < > '\" 😀")).unwrap();
    let encoded = x.encode_utf16le().unwrap();
    assert_eq!(&encoded[..2], &[255, 254]);
    let y = XmlInfo::parse_utf16le(&encoded).unwrap();
    assert_eq!(y.description(1), Some("雪 & < > '\" 😀"));
    assert!(
        y.to_xml()
            .unwrap()
            .contains("<CUSTOM a=\"&amp;\">one😀<X></X>two</CUSTOM>")
    );
}
#[test]
fn rejects_bad_indices_encryption_numeric_entities_and_excess_depth() {
    for s in [
        "<WIM><IMAGE/></WIM>",
        "<WIM><IMAGE INDEX='2'/></WIM>",
        "<WIM><IMAGE INDEX='1'/><IMAGE INDEX='1'/></WIM>",
        "<WIM><IMAGE INDEX='1'><NAME>&#65;</NAME></IMAGE></WIM>",
    ] {
        assert_eq!(XmlInfo::parse(s).unwrap_err(), ParseError::Xml);
    }
    assert_eq!(
        XmlInfo::parse("<WIM><ESD><ENCRYPTED/></ESD></WIM>").unwrap_err(),
        ParseError::WimIsEncrypted
    );
    assert_eq!(
        XmlInfo::parse(&format!("{}{}", "<WIM>".repeat(51), "</WIM>".repeat(51))).unwrap_err(),
        ParseError::Xml
    );
}
#[test]
fn setter_replaces_attributes_and_children_and_removal_bad_syntax_is_noop() {
    let mut x =
        XmlInfo::parse("<WIM><IMAGE INDEX='1'><A attr='x'><B/>text</A></IMAGE></WIM>").unwrap();
    x.set_property(1, "A", Some("replacement")).unwrap();
    assert!(x.to_xml().unwrap().contains("<A>replacement</A>"));
    assert_eq!(x.set_property(1, "/A", None), Ok(()));
    assert_eq!(
        x.set_property(1, "/A", Some("x")),
        Err(ParseError::InvalidParam)
    );
    assert_eq!(
        x.set_property(1, "1A", Some("x")),
        Err(ParseError::InvalidParam)
    );
}
#[test]
fn syntax_failure_retains_created_ancestors() {
    let mut x = XmlInfo::parse("<WIM><IMAGE INDEX='1'/></WIM>").unwrap();
    assert_eq!(
        x.set_property(1, "A/B/", Some("x")),
        Err(ParseError::InvalidParam)
    );
    assert!(x.to_xml().unwrap().contains("<A><B></B></A>"));
}
