fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // Upstream libtool 42:0:27 supplies the public ELF ABI name libwim.so.15.
        // Symbol and behavioral compatibility still require the recorded gates.
        println!("cargo:rustc-link-arg-cdylib=-Wl,-soname,libwim.so.15");
    }
}
