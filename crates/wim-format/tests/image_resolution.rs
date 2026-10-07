use wim_format::xml::XmlInfo;

#[test]
fn image_resolution_obeys_numeric_precedence_and_exact_names() {
    let xml = XmlInfo::parse("<WIM><IMAGE INDEX=\"1\"><NAME>0</NAME></IMAGE><IMAGE INDEX=\"2\"><NAME>3</NAME></IMAGE><IMAGE INDEX=\"3\"><NAME>Alpha</NAME></IMAGE></WIM>").unwrap();
    for (value, expected) in [
        ("all", -1),
        ("ALL", -1),
        ("*", -1),
        ("", 0),
        ("1", 1),
        (" +02", 2),
        ("3", 3),
        ("0", 1),
        ("Alpha", 3),
        ("alpha", 0),
        ("1 ", 0),
        ("4", 0),
        ("999999999999999999999999999999", 0),
    ] {
        assert_eq!(xml.resolve_image(Some(value)), expected, "{value:?}");
    }
    assert_eq!(xml.resolve_image(None), 0);
}
