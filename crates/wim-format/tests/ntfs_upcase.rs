#[test]
fn native_ntfs_uppercase_preserves_windows_specific_mappings() {
    use wim_format::ntfs_upcase::uppercase;
    assert_eq!(uppercase(b'a' as u16), b'A' as u16);
    assert_eq!(uppercase(0x00e9), 0x00c9);
    // This NTFS table retains final sigma; Unicode uppercase would differ.
    assert_eq!(uppercase(0x03c2), 0x03c2);
    assert_eq!(uppercase(0xd800), 0xd800);
    assert_eq!(uppercase(0xffff), 0xffff);
}
