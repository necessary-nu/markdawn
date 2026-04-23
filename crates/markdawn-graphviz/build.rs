//! Build script for markdawn-graphviz.
//!
//! By default we build the vendored graphviz submodule from source with
//! a minimal, hardened feature set (no Quartz, no Pango/Cairo, no
//! GD/WebP, no language bindings, no GUIs). Static linking. This gives
//! us a predictable, process-isolation-friendly graphviz across
//! Linux and macOS.
//!
//! Env override: if both `GRAPHVIZ_INCLUDE_DIR` and `GRAPHVIZ_LIB_DIR`
//! are set, we skip the vendored build and link against a pre-built
//! graphviz on those paths (useful for cross-compile targets).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/wrapper.c");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rerun-if-env-changed=GRAPHVIZ_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=GRAPHVIZ_LIB_DIR");
    println!("cargo:rerun-if-env-changed=MARKDAWN_GV_USE_SYSTEM");

    let (includes, lib_dirs, link_libs, link_kind) = if env::var_os("MARKDAWN_GV_USE_SYSTEM")
        .is_some()
        || (env::var_os("GRAPHVIZ_INCLUDE_DIR").is_some()
            && env::var_os("GRAPHVIZ_LIB_DIR").is_some())
    {
        resolve_system_graphviz()
    } else {
        build_vendored_graphviz()
    };

    let mut build = cc::Build::new();
    build.file("src/wrapper.c");
    for inc in &includes {
        build.include(inc);
    }
    apply_hardening_cflags(&mut build);
    build.compile("markdawn_graphviz_wrapper");

    for dir in &lib_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    for lib in &link_libs {
        println!("cargo:rustc-link-lib={}={}", link_kind, lib);
    }

    // Transitive platform libs that graphviz's static build depends on.
    // Only needed when link_kind == "static".
    if link_kind == "static" {
        // zlib for gvdevice's compressed output streams.
        println!("cargo:rustc-link-lib=dylib=z");
        #[cfg(target_os = "macos")]
        {
            println!("cargo:rustc-link-lib=dylib=c++");
        }
        #[cfg(target_os = "linux")]
        {
            println!("cargo:rustc-link-lib=dylib=m");
            println!("cargo:rustc-link-lib=dylib=dl");
            println!("cargo:rustc-link-lib=dylib=stdc++");
        }
    }
}

fn resolve_system_graphviz() -> (Vec<PathBuf>, Vec<PathBuf>, Vec<&'static str>, &'static str) {
    if let (Ok(inc), Ok(lib)) = (
        env::var("GRAPHVIZ_INCLUDE_DIR"),
        env::var("GRAPHVIZ_LIB_DIR"),
    ) {
        let includes = inc.split(':').map(PathBuf::from).collect::<Vec<_>>();
        let lib_dirs = lib.split(':').map(PathBuf::from).collect::<Vec<_>>();
        return (includes, lib_dirs, vec!["gvc", "cgraph", "cdt"], "dylib");
    }

    let gvc = pkg_config::Config::new()
        .probe("libgvc")
        .unwrap_or_else(|e| panic!("pkg-config libgvc: {e}"));
    let cgraph = pkg_config::Config::new()
        .probe("libcgraph")
        .unwrap_or_else(|e| panic!("pkg-config libcgraph: {e}"));
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
    (includes, lib_dirs, vec!["gvc", "cgraph", "cdt"], "dylib")
}

fn build_vendored_graphviz() -> (Vec<PathBuf>, Vec<PathBuf>, Vec<&'static str>, &'static str) {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let graphviz_src = manifest_dir.join("vendor/graphviz");
    if !graphviz_src.join("CMakeLists.txt").exists() {
        panic!(
            "graphviz submodule missing at {}. Run: git submodule update --init --recursive",
            graphviz_src.display()
        );
    }

    check_tool("cmake", "install CMake (brew install cmake / apt install cmake)");
    check_tool("bison", "install Bison (brew install bison / apt install bison)");
    check_tool("flex", "install Flex (brew install flex / apt install flex)");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let build_dir = out_dir.join("graphviz-build");
    let install_dir = out_dir.join("graphviz-install");
    std::fs::create_dir_all(&build_dir).expect("mkdir build_dir");

    // Configure. Minimal feature set, static libs, hardening.
    //
    // - WITH_QUARTZ=OFF: no Cocoa dependency on macOS (the whole
    //   reason we're vendoring — enables fork() isolation).
    // - WITH_PANGOCAIRO=OFF: disables Pango/Cairo; we only emit SVG
    //   (handled by the core plugin, not pangocairo), no font
    //   metrics via Cairo, smaller attack surface.
    // - WITH_GD/EXPAT/WEBP/RSVG/X=OFF: no raster codecs, no XML, no
    //   X11. Entire classes of CVE surface eliminated.
    // - ENABLE_*=OFF: no language bindings (Tcl/Java/Python/etc).
    // - BUILD_SHARED_LIBS=OFF: static linking, no dylib loader cost.
    // - use_*_ON: use system flex/bison.
    let mut cfg = Command::new("cmake");
    cfg.arg(&graphviz_src)
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install_dir.display()))
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON")
        // Hardened compile flags passed to graphviz's own sources.
        .arg(format!(
            "-DCMAKE_C_FLAGS_RELEASE={}",
            hardening_flags().join(" ")
        ))
        // Minimal plugin set.
        .arg("-DWITH_QUARTZ=OFF")
        .arg("-DWITH_PANGOCAIRO=OFF")
        .arg("-DWITH_GD=OFF")
        .arg("-DWITH_GDK_PIXBUF=OFF")
        .arg("-DWITH_EXPAT=OFF")
        // Graphviz's CMakeLists has no WITH_GTS flag; it unconditionally
        // `find_package(GTS)`s and uses it if present. Tell CMake not to
        // even look, so we don't accidentally link against a
        // glib-dependent GTS package installed on a dev machine.
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_GTS=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_Pango=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_Cairo=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_EXPAT=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_GD=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_RSVG=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_DevIL=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_Quartz=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_X11=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_WEBP=TRUE")
        .arg("-DCMAKE_DISABLE_FIND_PACKAGE_GLIB=TRUE")
        .arg("-DWITH_LIBGD=OFF")
        .arg("-DWITH_WEBP=OFF")
        .arg("-DWITH_X=OFF")
        .arg("-DWITH_ZLIB=ON") // Needed for compressed output paths used internally.
        .arg("-DWITH_GVEDIT=OFF")
        .arg("-DWITH_SMYRNA=OFF")
        // Graphviz uses lowercase names for some options — match
        // what CMakeLists.txt actually declares.
        .arg("-Dwith_ortho=OFF")
        .arg("-Dwith_sfdp=OFF")
        .arg("-DUSE_LTDL=OFF") // Skip libltdl; we don't need demand-loading plugins.
        .arg("-DENABLE_LTDL=OFF")
        .arg("-DENABLE_SWIG=OFF")
        .arg("-DENABLE_TCL=OFF")
        .arg("-DENABLE_PERL=OFF")
        .arg("-DENABLE_PYTHON=OFF")
        .arg("-DENABLE_RUBY=OFF")
        .arg("-DENABLE_GO=OFF")
        .arg("-DENABLE_JAVA=OFF")
        .arg("-DENABLE_LUA=OFF")
        .arg("-DENABLE_R=OFF")
        .arg("-DENABLE_SHARP=OFF")
        .arg("-DENABLE_PHP=OFF")
        .arg("-DENABLE_D=OFF")
        .arg("-DENABLE_GUILE=OFF")
        .arg("-DENABLE_IO=OFF")
        .arg("-DENABLE_JAVASCRIPT=OFF")
        .arg("-DENABLE_OCAML=OFF")
        // Tests and docs off.
        .arg("-DBUILD_TESTING=OFF")
        .arg("-DBUILD_SHARP_GVTEST=OFF");

    // Tell CMake where Homebrew's flex / bison live on macOS —
    // /usr/bin/flex on macOS is ancient and graphviz wants a newer one.
    if cfg!(target_os = "macos") {
        if Path::new("/opt/homebrew/opt/bison/bin/bison").exists() {
            cfg.arg("-DBISON_EXECUTABLE=/opt/homebrew/opt/bison/bin/bison");
        }
        if Path::new("/opt/homebrew/opt/flex/bin/flex").exists() {
            cfg.arg("-DFLEX_EXECUTABLE=/opt/homebrew/opt/flex/bin/flex");
        }
    }

    let cfg_status = cfg
        .current_dir(&build_dir)
        .status()
        .expect("run cmake configure");
    if !cfg_status.success() {
        panic!("cmake configure for vendored graphviz failed");
    }

    // Build only the targets we actually need. We force the `dot`
    // engine in render_inner, so none of the other layout libs
    // (neatogen / circogen / fdpgen / sfdpgen / twopigen / osage /
    // patchwork / ortho) need to exist. gvpr is the Graphviz script
    // language — useful CLI tool, unnecessary attack surface for us.
    // The CLI binaries in cmd/ (dot, gvpack, etc.) aren't built
    // either, which also dodges their link-against-disabled-deps
    // problems.
    let targets: &[&str] = &[
        // Core libraries.
        "cdt",
        "cgraph",
        "common",
        "pathplan",
        "pack",
        "label",
        "xdot",
        "util",
        "sparse",
        "vpsc",
        "rbtree",
        "ast",
        "expr",
        "sfio",
        // Dot layout engine.
        "dotgen",
        // Context glue.
        "gvc",
        // Minimal plugin set.
        "gvplugin_core",
        "gvplugin_dot_layout",
    ];
    let jobs = env::var("NUM_JOBS").unwrap_or_else(|_| "4".to_owned());
    let mut build_cmd = Command::new("cmake");
    build_cmd
        .arg("--build")
        .arg(".")
        .arg("--parallel")
        .arg(&jobs);
    for t in targets {
        build_cmd.arg("--target").arg(t);
    }
    let build_status = build_cmd
        .current_dir(&build_dir)
        .status()
        .expect("run cmake --build");
    if !build_status.success() {
        panic!("cmake build for vendored graphviz failed");
    }

    // We use `#include <graphviz/gvc.h>` in wrapper.c (the public
    // convention), so stage the handful of public headers under
    // OUT_DIR/include/graphviz/ and pass that as the include root.
    let staged_include = out_dir.join("include");
    let staged_graphviz = staged_include.join("graphviz");
    std::fs::create_dir_all(&staged_graphviz).expect("mkdir staged include");
    let header_sources = [
        ("lib/gvc/gvc.h", "gvc.h"),
        ("lib/gvc/gvcext.h", "gvcext.h"),
        ("lib/gvc/gvcommon.h", "gvcommon.h"),
        ("lib/gvc/gvplugin.h", "gvplugin.h"),
        ("lib/gvc/gvplugin_device.h", "gvplugin_device.h"),
        ("lib/gvc/gvplugin_layout.h", "gvplugin_layout.h"),
        ("lib/gvc/gvplugin_loadimage.h", "gvplugin_loadimage.h"),
        ("lib/gvc/gvplugin_render.h", "gvplugin_render.h"),
        ("lib/gvc/gvplugin_textlayout.h", "gvplugin_textlayout.h"),
        ("lib/gvc/gvconfig.h", "gvconfig.h"),
        ("lib/cgraph/cgraph.h", "cgraph.h"),
        ("lib/cdt/cdt.h", "cdt.h"),
        ("lib/pathplan/pathplan.h", "pathplan.h"),
        ("lib/pathplan/pathgeom.h", "pathgeom.h"),
        ("lib/pathplan/pathrender.h", "pathrender.h"),
        ("lib/pathplan/vispath.h", "vispath.h"),
        ("lib/xdot/xdot.h", "xdot.h"),
        ("lib/common/color.h", "color.h"),
        ("lib/common/textspan.h", "textspan.h"),
        ("lib/common/types.h", "types.h"),
        ("lib/common/usershape.h", "usershape.h"),
    ];
    for (src_rel, dst_name) in header_sources {
        let src_path = graphviz_src.join(src_rel);
        if src_path.exists() {
            let dst_path = staged_graphviz.join(dst_name);
            std::fs::copy(&src_path, &dst_path).unwrap_or_else(|e| {
                panic!("copy header {}: {e}", src_path.display())
            });
        }
    }
    // Some graphviz public headers transitively `#include "other.h"`
    // (relative path, no `graphviz/` prefix). Add each subdir as an
    // include root so those transitive includes resolve.
    let includes = vec![
        staged_include,
        graphviz_src.join("lib"),
        graphviz_src.join("lib/common"),
        graphviz_src.join("lib/cdt"),
        graphviz_src.join("lib/cgraph"),
        graphviz_src.join("lib/gvc"),
        graphviz_src.join("lib/pathplan"),
        graphviz_src.join("lib/util"),
        graphviz_src.join("lib/xdot"),
        build_dir.to_path_buf(),       // config.h generated by cmake
        build_dir.join("lib"),
        build_dir.join("lib/common"),  // colortbl.h etc.
    ];

    // Link libs in a specific order: plugins → layout engine → gvc →
    // utility libs → core. Static archives resolve top-down, so
    // upstream deps come after downstream references.
    let mut lib_dirs: Vec<PathBuf> = Vec::new();
    let mut archives: Vec<String> = Vec::new();
    let desired_order: &[&str] = &[
        "gvplugin_core",
        "gvplugin_dot_layout",
        "dotgen",
        "gvc",
        "pack",
        "label",
        "common",
        "xdot",
        "pathplan",
        "vpsc",
        "sparse",
        "rbtree",
        "ast",
        "expr",
        "sfio",
        "util",
        "cgraph",
        "cdt",
    ];
    for name in desired_order {
        let archive = find_archive(&build_dir, &format!("lib{name}.a"));
        if let Some(path) = archive {
            let dir = path.parent().unwrap().to_path_buf();
            if !lib_dirs.contains(&dir) {
                lib_dirs.push(dir);
            }
            archives.push((*name).to_owned());
        } else {
            panic!("expected graphviz archive lib{name}.a not found under {}", build_dir.display());
        }
    }

    let link_libs_static: Vec<&'static str> = archives
        .into_iter()
        .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
        .collect();
    (includes, lib_dirs, link_libs_static, "static")
}

fn find_archive(build_dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in walk_archives(build_dir) {
        if entry.file_name().and_then(|s| s.to_str()) == Some(name) {
            return Some(entry);
        }
    }
    None
}

fn walk_archives(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk_archives(&p));
        } else if p.extension().and_then(|e| e.to_str()) == Some("a") {
            out.push(p);
        }
    }
    out
}

fn hardening_flags() -> Vec<&'static str> {
    let mut v = vec![
        "-O2",
        "-fstack-protector-strong",
        "-D_FORTIFY_SOURCE=2",
        "-Wformat",
        "-Wformat-security",
        "-fPIC",
    ];
    if cfg!(target_os = "linux") {
        v.push("-fstack-clash-protection");
    }
    if cfg!(target_arch = "x86_64") {
        v.push("-fcf-protection=full");
    }
    v
}

fn apply_hardening_cflags(build: &mut cc::Build) {
    for flag in hardening_flags() {
        build.flag_if_supported(flag);
    }
}

fn check_tool(name: &str, install_hint: &str) {
    if Command::new(name).arg("--version").output().is_err() {
        panic!("required tool `{name}` not found in PATH. {install_hint}");
    }
}
