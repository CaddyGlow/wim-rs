/// Encode test strings using the actual platform C API character width.
pub fn text(value: &str) -> Vec<wim::ffi::TChar> {
    assert!(!value.contains('\0'));
    #[cfg(windows)]
    {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
    #[cfg(not(windows))]
    {
        value
            .bytes()
            .map(|byte| byte as wim::ffi::TChar)
            .chain(std::iter::once(0))
            .collect()
    }
}
