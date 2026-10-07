#[test]
fn reads_checked_in_wim_fixtures() {
    for bytes in [
        include_bytes!("../../crates/wim-format/tests/fixtures/xpress-resource.wim").as_slice(),
        include_bytes!("../../crates/wim-format/tests/fixtures/pipe-none.wim").as_slice(),
    ] {
        wim_fuzz::wim(bytes);
    }
}
