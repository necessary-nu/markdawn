//! Minimal `typst::World` for rendering one fenced `typst` code block.
//!
//! The world is **hermetic**: there is no network, no real filesystem,
//! no clock. The only inputs are the user's source string and the
//! bundled package tree compiled into the binary via [`include_dir!`].
//!
//! Layout under `assets/typst-packages/preview/{name}/{version}/...`
//! matches what typst's CLI expects in its package cache, so we can
//! drop in a vendored tarball verbatim.

use std::path::Path;
use std::sync::OnceLock;

use ecow::EcoString;
use include_dir::{Dir, include_dir};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration as TypstDuration};
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// All bundled packages, mounted under `preview/`.
static BUNDLED_PACKAGES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/typst-packages");

/// Process-wide library + font book + fonts, built once.
// [spec:mw:def:typst.world.hermetic]
struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

// [spec:mw:def:typst.world.hermetic]
fn shared() -> &'static Shared {
    static CELL: OnceLock<Shared> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut fonts: Vec<Font> = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data);
            // A single font file may contain multiple faces (TTC). Walk
            // the index until `Font::new` returns None.
            let mut index = 0u32;
            while let Some(font) = Font::new(bytes.clone(), index) {
                fonts.push(font);
                index += 1;
            }
        }
        let book = FontBook::from_fonts(&fonts);
        Shared {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
        }
    })
}

/// World for a single render pass. Holds the user's main source plus a
/// cache of any package source files touched during compilation.
pub(crate) struct RenderWorld {
    main_id: FileId,
    main_source: Source,
}

impl RenderWorld {
    pub(crate) fn new(source: String) -> Self {
        let main_id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("main.typ").expect("static main path is valid"),
        ));
        let main_source = Source::new(main_id, source);
        Self {
            main_id,
            main_source,
        }
    }
}

// [spec:mw:def:typst.world.hermetic]
impl World for RenderWorld {
    fn library(&self) -> &LazyHash<Library> {
        &shared().library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &shared().book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_id {
            return Ok(self.main_source.clone());
        }
        let bytes = read_package_file(id)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| FileError::InvalidUtf8)?
            .to_owned();
        Ok(Source::new(id, text))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main_id {
            return Ok(Bytes::new(self.main_source.text().to_owned().into_bytes()));
        }
        let bytes = read_package_file(id)?;
        Ok(Bytes::new(bytes.to_vec()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        shared().fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<TypstDuration>) -> Option<Datetime> {
        // Fixed epoch keeps renders deterministic.
        Datetime::from_ymd(2024, 1, 1)
    }
}

/// Resolve a `FileId` to bundled bytes. Only `@preview/...` packages we
/// ship are reachable; everything else maps to `FileError::NotFound`.
// [spec:mw:req:typst.packages.offline-resolution]
fn read_package_file(id: FileId) -> FileResult<&'static [u8]> {
    let VirtualRoot::Package(spec) = id.root() else {
        return Err(FileError::NotFound(
            Path::new(id.vpath().get_without_slash()).into(),
        ));
    };
    let inner = Path::new(id.vpath().get_without_slash());
    let rel = bundled_package_relative_path(spec, inner)
        .ok_or_else(|| FileError::NotFound(inner.into()))?;
    BUNDLED_PACKAGES
        .get_file(&rel)
        .map(|f| f.contents())
        .ok_or_else(|| FileError::NotFound(rel.into()))
}

/// Build the `preview/{name}/{version}/{path}` lookup key inside the
/// bundled `Dir`. Returns `None` for unknown namespaces.
// [spec:mw:req:typst.packages.offline-resolution]
fn bundled_package_relative_path(spec: &PackageSpec, inner: &Path) -> Option<std::path::PathBuf> {
    if spec.namespace.as_str() != "preview" {
        return None;
    }
    if !is_bundled(&spec.name, &spec.version.to_string()) {
        return None;
    }
    let mut p = std::path::PathBuf::from("preview");
    p.push(spec.name.as_str());
    p.push(spec.version.to_string());
    for part in inner.components() {
        if let std::path::Component::Normal(c) = part {
            p.push(c);
        }
    }
    Some(p)
}

/// Names + versions we ship. Must match the on-disk vendor tree under
/// `assets/typst-packages/preview/`. `oxifmt` is an indirect dep of
/// `cetz` — bundle it so cetz can resolve `#import "@preview/oxifmt:..."`.
// [spec:mw:req:typst.packages.offline-resolution]
const BUNDLED: &[(&str, &str)] = &[("syntree", "0.2.1"), ("cetz", "0.5.0"), ("oxifmt", "1.0.0")];

fn is_bundled(name: &EcoString, version: &str) -> bool {
    BUNDLED
        .iter()
        .any(|(n, v)| name.as_str() == *n && version == *v)
}
