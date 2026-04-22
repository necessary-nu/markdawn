use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/wrapper.c");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rerun-if-env-changed=GRAPHVIZ_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=GRAPHVIZ_LIB_DIR");

    let (includes, lib_dirs) = if let (Ok(inc), Ok(lib)) = (
        env::var("GRAPHVIZ_INCLUDE_DIR"),
        env::var("GRAPHVIZ_LIB_DIR"),
    ) {
        (
            inc.split(':').map(PathBuf::from).collect::<Vec<_>>(),
            lib.split(':').map(PathBuf::from).collect::<Vec<_>>(),
        )
    } else {
        // pkg-config handles host builds and cross-builds where
        // PKG_CONFIG_PATH_<target> is set. For cross-builds without
        // pkg-config metadata (most iOS setups), set
        // GRAPHVIZ_INCLUDE_DIR + GRAPHVIZ_LIB_DIR instead.
        let gvc = pkg_config::Config::new().probe("libgvc").unwrap_or_else(|e| {
            panic!(
                "could not locate libgvc. install graphviz (brew install graphviz / \
                 apt install graphviz-dev) or set GRAPHVIZ_INCLUDE_DIR + \
                 GRAPHVIZ_LIB_DIR. pkg-config error: {e}"
            )
        });
        let cgraph = pkg_config::Config::new()
            .probe("libcgraph")
            .unwrap_or_else(|e| panic!("could not locate libcgraph. pkg-config error: {e}"));
        let includes = gvc
            .include_paths
            .iter()
            .chain(cgraph.include_paths.iter())
            .cloned()
            .collect::<Vec<_>>();
        let lib_dirs = gvc
            .link_paths
            .iter()
            .chain(cgraph.link_paths.iter())
            .cloned()
            .collect::<Vec<_>>();
        (includes, lib_dirs)
    };

    let mut build = cc::Build::new();
    build.file("src/wrapper.c");
    for inc in &includes {
        build.include(inc);
    }
    build.compile("markdawn_graphviz_wrapper");

    for dir in &lib_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    // Link graphviz dylibs. pkg-config's probe already emits these for
    // the host path; we re-emit so the env-override path works too.
    // Idempotent — cargo dedupes duplicate rustc-link-lib entries.
    println!("cargo:rustc-link-lib=dylib=gvc");
    println!("cargo:rustc-link-lib=dylib=cgraph");
    println!("cargo:rustc-link-lib=dylib=cdt");
}
