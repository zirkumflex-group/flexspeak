fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // WebRTC uses Objective-C categories that have no ordinary linker references.
        println!("cargo:rustc-link-arg=-ObjC");
    }
}
