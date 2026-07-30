//! Dump arborium's highlight spans over a fixture corpus, for offline
//! comparison against the same corpus run through ntreesitter.
//!
//! The two backends declare the same Cargo `links = "tree-sitter"` identity,
//! so they cannot be linked into one process and the comparison cannot be a
//! test. It is two dumps, diffed. This is the arborium half; it exists only
//! until the backend swap lands, at which point arborium stops building here
//! and the baseline it produces becomes unobtainable.
//!
//! Usage:
//!   cargo run --release --example arborium_span_dump -- <languages-dir> <out>
//!
//! `<languages-dir>` is ntreesitter's `languages/`, whose per-language
//! `fixtures/` directories are the shared corpus. The language id is the
//! directory name, which is set-equal across the two catalogs.

use std::io::Write as _;
use std::path::Path;

use markdawn::SyntaxHighlighter;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(languages_dir), Some(out_path)) = (args.next(), args.next()) else {
        eprintln!("usage: arborium_span_dump <languages-dir> <out-file>");
        std::process::exit(2);
    };

    let mut entries: Vec<_> = std::fs::read_dir(&languages_dir)
        .unwrap_or_else(|error| panic!("read {languages_dir}: {error}"))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();

    let mut out = std::io::BufWriter::new(
        std::fs::File::create(&out_path)
            .unwrap_or_else(|error| panic!("create {out_path}: {error}")),
    );
    let mut highlighter = markdawn::arborium::Highlighter::new();
    let (mut ok, mut failed) = (0usize, 0usize);

    for language in entries {
        let fixtures = Path::new(&languages_dir).join(&language).join("fixtures");
        let mut files = Vec::new();
        collect_fixtures(&fixtures, &fixtures, &mut files);
        files.sort();

        for file in files {
            let source = match std::fs::read_to_string(fixtures.join(&file)) {
                Ok(source) => source,
                // A fixture that is not UTF-8 is not a highlighting result;
                // record it so the other side's dump lines up rather than
                // silently omitting the entry.
                Err(error) => {
                    writeln!(out, "## {language}/{file}").unwrap();
                    writeln!(out, "!unreadable {error}").unwrap();
                    failed += 1;
                    continue;
                }
            };

            writeln!(out, "## {language}/{file}").unwrap();
            // Explicit UFCS: arborium's inherent `highlight` returns rendered
            // HTML and would shadow the trait method. Going through markdawn's
            // boundary is also the more faithful comparison — `SyntaxSpan` is
            // what markdawn actually ships to renderers.
            match SyntaxHighlighter::highlight(&mut highlighter, &language, &source) {
                Ok(spans) => {
                    for span in spans {
                        writeln!(
                            out,
                            "{} {} {} {}",
                            span.start, span.end, span.pattern_index, span.capture
                        )
                        .unwrap();
                    }
                    ok += 1;
                }
                Err(error) => {
                    writeln!(out, "!error {error}").unwrap();
                    failed += 1;
                }
            }
        }
    }

    out.flush().unwrap();
    eprintln!("arborium: {ok} fixtures highlighted, {failed} failed");
}

/// Collect fixture paths relative to `root`, descending into subdirectories.
///
/// Most languages keep fixtures flat, but at least one (`silo`) nests them
/// under `fixtures/corpus/`; a non-recursive walk skips that language
/// entirely while still reporting success.
fn collect_fixtures(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_fixtures(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().into_owned());
        }
    }
}
