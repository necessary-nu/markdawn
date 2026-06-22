fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "emscripten" {
        return;
    }

    let exports = ["_mw_render_pikchr", "_mw_string_free", "_malloc", "_free"].join(",");
    let runtime_methods = [
        "cwrap",
        "ccall",
        "UTF8ToString",
        "stringToUTF8",
        "lengthBytesUTF8",
        "HEAPU8",
    ]
    .join(",");

    println!("cargo:rustc-link-arg=-sEXPORTED_FUNCTIONS={exports}");
    println!("cargo:rustc-link-arg=-sEXPORTED_RUNTIME_METHODS={runtime_methods}");
    println!("cargo:rustc-link-arg=-sMODULARIZE=1");
    println!("cargo:rustc-link-arg=-sEXPORT_NAME=createMarkdawnPikchr");
    println!("cargo:rustc-link-arg=-sEXPORT_ES6=1");
    println!("cargo:rustc-link-arg=-sALLOW_MEMORY_GROWTH=1");
    println!("cargo:rustc-link-arg=-sSTACK_SIZE=16777216");
    println!("cargo:rustc-link-arg=-sINITIAL_MEMORY=33554432");
    println!("cargo:rustc-link-arg=-sENVIRONMENT=web");
    println!("cargo:rerun-if-changed=build.rs");
}
