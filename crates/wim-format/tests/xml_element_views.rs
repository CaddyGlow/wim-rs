use wim_format::xml::XmlInfo;

#[test]
fn element_views_distinguish_absent_empty_and_repeated_text_children() {
    let xml = XmlInfo::parse("<WIM><IMAGE INDEX=\"1\"><WINDOWS><LANGUAGES><LANGUAGE>en-US</LANGUAGE><LANGUAGE/><OTHER>skip</OTHER><LANGUAGE>fr-FR</LANGUAGE></LANGUAGES><VERSION/></WINDOWS></IMAGE></WIM>").unwrap();
    assert!(xml.has_element(1, b"WINDOWS/VERSION"));
    assert!(!xml.has_element(1, b"WINDOWS/MISSING"));
    assert_eq!(xml.get_property_bytes(1, b"WINDOWS/VERSION"), None);
    let languages: Vec<_> = xml.child_elements(1, b"WINDOWS/LANGUAGES").collect();
    assert_eq!(
        languages,
        [
            (b"LANGUAGE".as_slice(), Some(b"en-US".as_slice())),
            (b"LANGUAGE".as_slice(), None),
            (b"OTHER".as_slice(), Some(b"skip".as_slice())),
            (b"LANGUAGE".as_slice(), Some(b"fr-FR".as_slice()))
        ]
    );
    assert_eq!(xml.child_elements(2, b"WINDOWS").count(), 0);
}
