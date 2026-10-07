fn main() {
    loop {
        honggfuzz::fuzz!(|data: &[u8]| {
            wim_fuzz::wim(data);
        });
    }
}
