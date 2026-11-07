fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target == "macos" {
        // WebRTC uses Objective-C categories that have no ordinary linker references.
        println!("cargo:rustc-link-arg=-ObjC");
    }
    if target == "windows" {
        // The executable's icon (assets/icon/flexspeak.ico, generated from flexspeak.svg).
        println!("cargo:rerun-if-changed=assets/icon/flexspeak.rc");
        println!("cargo:rerun-if-changed=assets/icon/flexspeak.ico");
        embed_resource::compile("assets/icon/flexspeak.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("compile the Windows icon resource");
    }
    println!("cargo:rerun-if-changed=native/audio.cpp");
    println!("cargo:rerun-if-changed=native/camera.cpp");
    println!("cargo:rerun-if-changed=native/camera.mm");
    println!("cargo:rerun-if-env-changed=LK_CUSTOM_WEBRTC");
    println!("cargo:rerun-if-env-changed=LK_DEBUG_WEBRTC");
    if !matches!(target.as_str(), "macos" | "windows") {
        return;
    }
    // Reuse exactly the headers, defines and binary already selected by webrtc-sys.
    webrtc_sys_build::download_webrtc().expect("Cannot prepare the pinned WebRTC library");
    let directory = webrtc_sys_build::webrtc_dir();
    let include = directory.join("include");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .file("native/audio.cpp")
        .include(&include)
        .include(include.join("third_party/abseil-cpp"));
    for (key, value) in webrtc_sys_build::webrtc_defines() {
        build.define(&key, value.as_deref());
    }
    build.define("WEBRTC_APM_DEBUG_DUMP", "0");
    build
        .flag_if_supported("-Wno-nullability-completeness")
        .flag_if_supported("-Wno-unused-parameter");
    if target == "windows" {
        build
            .flag("/EHsc")
            .file("native/camera.cpp")
            .include(include.join("third_party/libyuv/include"));
    }
    if target == "macos" {
        build.flag("-stdlib=libc++");
    }
    build.compile("flexspeak_audio");
    if target == "macos" {
        cc::Build::new()
            .cpp(true)
            .std("c++20")
            .file("native/camera.mm")
            .flag("-fobjc-arc")
            .flag("-stdlib=libc++")
            .compile("flexspeak_camera");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
    }
    println!(
        "cargo:rustc-link-search=native={}",
        directory.join("lib").display()
    );
}
