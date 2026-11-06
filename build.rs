fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS");
    if target_os.as_deref() == Ok("macos") {
        // WebRTC uses Objective-C categories that have no ordinary linker references.
        println!("cargo:rustc-link-arg=-ObjC");
    }
    if target_os.as_deref() == Ok("windows") {
        // The executable's icon (assets/icon/flexspeak.ico, generated from flexspeak.svg).
        println!("cargo:rerun-if-changed=assets/icon/flexspeak.rc");
        println!("cargo:rerun-if-changed=assets/icon/flexspeak.ico");
        embed_resource::compile("assets/icon/flexspeak.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("compile the Windows icon resource");
    }
}
