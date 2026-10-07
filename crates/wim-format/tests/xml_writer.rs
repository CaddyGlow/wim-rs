use wim_format::{ParseError, xml::XmlInfo};
#[test]
fn byte_statistics_update_does_not_invalidate_image_positions() {
    let mut xml=XmlInfo::parse("<WIM><TOTALBYTES>123</TOTALBYTES><IMAGE INDEX=\"1\"><NAME>A</NAME></IMAGE><CUSTOM>retain</CUSTOM></WIM>").unwrap();
    assert_eq!(xml.total_bytes(), 123);
    xml.set_total_bytes(None).unwrap();
    assert_eq!(xml.total_bytes(), 0);
    assert_eq!(xml.name(1), Some("A"));
    xml.set_total_bytes(Some(u64::MAX - 1)).unwrap();
    assert_eq!(xml.total_bytes(), u64::MAX - 1);
    xml.set_total_bytes(Some(u64::MAX)).unwrap();
    assert_eq!(xml.total_bytes(), 0);
    assert!(xml.to_xml().unwrap().contains("<CUSTOM>retain</CUSTOM>"));
}
#[test]
fn selected_images_preserve_unknown_data_and_renumber_indices() {
    let xml=XmlInfo::parse("<WIM><UNKNOWN k=\"v\"/><IMAGE INDEX=\"1\"><NAME>A</NAME></IMAGE><IMAGE INDEX=\"2\"><NAME>B</NAME><EXTRA>yes</EXTRA></IMAGE></WIM>").unwrap();
    let selected = xml.select_images(&[2, 1]).unwrap();
    assert_eq!(selected.name(1), Some("B"));
    assert_eq!(selected.name(2), Some("A"));
    assert_eq!(selected.get_property(1, "EXTRA"), Some("yes"));
    assert!(
        selected
            .to_xml()
            .unwrap()
            .contains("<UNKNOWN k=\"v\"></UNKNOWN>")
    );
    assert_eq!(
        XmlInfo::parse(&selected.to_xml().unwrap())
            .unwrap()
            .image_count(),
        2
    );
    assert_eq!(xml.select_images(&[0]), Err(ParseError::InvalidImage));
    assert_eq!(xml.select_images(&[3]), Err(ParseError::InvalidImage));
    assert_eq!(xml.select_images(&[]).unwrap().image_count(), 0);
}
