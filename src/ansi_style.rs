//! Shared SGR → CSS translation for the `ansi` and `terminal` features.
//!
//! Both `ansi_to_html` (byte-stream → HTML) and `terminal` (VT100 grid
//! → HTML) need the same mapping of xterm color palettes and SGR
//! attributes onto inline CSS. Keeping one authoritative copy here
//! means a palette tweak ripples to both pipelines.

/// 4-bit / 8-bit / 24-bit color, matching xterm's SGR 30-37, 90-97,
/// 38;5, 38;2 conventions.
// `Named` stays distinct from `Palette(0..=15)` so the SGR re-encoding
// direction can recover the short 30-37 / 90-97 forms.
// [spec:mw:def:ansi.style.color]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    /// 0–7 (normal) and 8–15 (bright); palette resolved at render time.
    Named(u8),
    /// 256-color palette index (0–255).
    Palette(u8),
    /// Truecolor (24-bit R/G/B).
    Rgb(u8, u8, u8),
}

impl Color {
    pub fn to_css(self) -> String {
        match self {
            Color::Named(n) => named_palette(n).into(),
            Color::Palette(n) => ansi256_css(n),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// xterm default palette (brights on 8–15).
// Out-of-range indices fall back to `inherit` — the ambient colour is a
// safer wrong answer than an arbitrary swatch.
// [spec:mw:sem:ansi.style.palette-resolution]
pub fn named_palette(n: u8) -> &'static str {
    match n {
        0 => "#000000",
        1 => "#cd0000",
        2 => "#00cd00",
        3 => "#cdcd00",
        4 => "#0000ee",
        5 => "#cd00cd",
        6 => "#00cdcd",
        7 => "#e5e5e5",
        8 => "#7f7f7f",
        9 => "#ff0000",
        10 => "#00ff00",
        11 => "#ffff00",
        12 => "#5c5cff",
        13 => "#ff00ff",
        14 => "#00ffff",
        15 => "#ffffff",
        _ => "inherit",
    }
}

/// RGB channels behind a named-palette entry, or `None` for an index the
/// table leaves to the ambient colour.
// Reads the one authoritative table in `named_palette` rather than
// restating it, so `inherit` naturally has no triple.
// [spec:mw:sem:ansi.style.palette-resolution]
pub fn named_rgb(n: u8) -> Option<(u8, u8, u8)> {
    let hex = named_palette(n).strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// RGB channels behind an xterm 256-color index, using the same
/// three-way split as [`ansi256_css`].
// [spec:mw:sem:ansi.style.palette-resolution]
pub fn palette_rgb(n: u8) -> (u8, u8, u8) {
    match n {
        0..=15 => named_rgb(n).unwrap_or((0, 0, 0)),
        16..=231 => {
            let idx = n - 16;
            let level = |v: u8| if v == 0 { 0u8 } else { 55 + v * 40 };
            (level((idx / 36) % 6), level((idx / 6) % 6), level(idx % 6))
        }
        232..=255 => {
            let shade = 8 + (n - 232) * 10;
            (shade, shade, shade)
        }
    }
}

/// Translate an xterm 256-color index to a CSS colour. Indices 0–15 are
/// the named palette; 16–231 are a 6x6x6 colour cube; 232–255 are a
/// 24-step greyscale ramp.
// [spec:mw:sem:ansi.style.palette-resolution]
pub fn ansi256_css(n: u8) -> String {
    match n {
        0..=15 => named_palette(n).into(),
        _ => {
            let (r, g, b) = palette_rgb(n);
            format!("#{r:02x}{g:02x}{b:02x}")
        }
    }
}

fn rgb_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let square = |x: u8, y: u8| {
        let diff = i32::from(x) - i32::from(y);
        (diff * diff) as u32
    };
    square(a.0, b.0) + square(a.1, b.1) + square(a.2, b.2)
}

/// Nearest xterm 256-color index for a truecolour triple.
// The search covers 16–255: the 6x6x6 cube and the greyscale ramp. The
// 16 base entries are excluded because a terminal is free to retheme
// them, so quantising onto one would not reproduce the requested hue.
// [spec:mw:req:ansi.adapter.color-depth-inert+1]
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    let target = (r, g, b);
    let mut best = 16u8;
    let mut best_distance = u32::MAX;
    for index in 16..=255u8 {
        let distance = rgb_distance(target, palette_rgb(index));
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// Nearest named-palette entry for a truecolour triple.
// [spec:mw:req:ansi.adapter.color-depth-inert+1]
pub fn rgb_to_named(r: u8, g: u8, b: u8) -> u8 {
    let target = (r, g, b);
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for index in 0..16u8 {
        let Some(entry) = named_rgb(index) else {
            continue;
        };
        let distance = rgb_distance(target, entry);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// Nearest named-palette entry for a 256-color index. Indices below 16
/// already name an entry and pass through untouched.
// [spec:mw:req:ansi.adapter.color-depth-inert+1]
pub fn ansi256_to_named(n: u8) -> u8 {
    if n < 16 {
        return n;
    }
    let (r, g, b) = palette_rgb(n);
    rgb_to_named(r, g, b)
}

/// SGR parameter text for one colour, as a foreground or a background.
// [spec:mw:syn:ansi.term.ansi-encoding]
pub fn color_to_sgr(c: Color, bg: bool) -> String {
    let base: u16 = if bg { 40 } else { 30 };
    match c {
        Color::Named(n) if n < 8 => format!("{}", base + u16::from(n)),
        Color::Named(n) => format!("{}", base + 60 + u16::from(n - 8)),
        Color::Palette(n) => format!("{};5;{n}", base + 8),
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
    }
}

/// Terminal cell style — full SGR attribute set plus fg/bg colors. Both
/// the byte-stream and grid-snapshot pipelines work with this shape.
// `Default` is the plain style; `is_plain` is equality with it, and is
// the single predicate both renderers use to decide whether any
// styling wrapper is needed at all.
// [spec:mw:def:ansi.style.cell-style]
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub inverse: bool,
}

impl CellStyle {
    /// CSS declaration string for this style (no trailing semicolon).
    /// `inverse` swaps fg/bg so the HTML mirrors what a terminal would
    /// actually show.
    // Declaration order is fixed so equal styles stringify identically
    // and run-coalescing can compare on the style, not the text.
    // underline+strike must collapse into one `text-decoration`: two
    // declarations would leave only the last one live.
    // [spec:mw:req:ansi.style.css-emission]
    pub fn css(&self) -> String {
        let mut out = String::new();
        let mut push = |prop: &str, value: &str| {
            if !out.is_empty() {
                out.push(';');
            }
            out.push_str(prop);
            out.push(':');
            out.push_str(value);
        };
        let (fg, bg) = if self.inverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        };
        if let Some(c) = fg {
            push("color", &c.to_css());
        }
        if let Some(c) = bg {
            push("background", &c.to_css());
        }
        if self.bold {
            push("font-weight", "700");
        }
        if self.dim {
            push("opacity", "0.6");
        }
        if self.italic {
            push("font-style", "italic");
        }
        if self.underline && self.strike {
            push("text-decoration", "underline line-through");
        } else if self.underline {
            push("text-decoration", "underline");
        } else if self.strike {
            push("text-decoration", "line-through");
        }
        out
    }

    pub fn is_plain(&self) -> bool {
        *self == CellStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:mw:sem:ansi.style.palette-resolution/test]
    #[test]
    fn palette_rgb_hits_each_region_boundary() {
        assert_eq!(palette_rgb(0), (0, 0, 0));
        assert_eq!(palette_rgb(15), (255, 255, 255));
        // Cube corners: first entry is black, last is white.
        assert_eq!(palette_rgb(16), (0, 0, 0));
        assert_eq!(palette_rgb(231), (255, 255, 255));
        // Cube levels are 0 then 55 + 40v, so (1, 2, 3) is 95/135/175.
        assert_eq!(palette_rgb(67), (95, 135, 175));
        // Grey ramp runs 8..=238 in steps of 10 across 24 entries.
        assert_eq!(palette_rgb(232), (8, 8, 8));
        assert_eq!(palette_rgb(255), (238, 238, 238));
        assert_eq!(named_rgb(16), None);
    }

    // [spec:mw:req:ansi.adapter.color-depth-inert+1/test]
    #[test]
    fn truecolor_quantises_onto_the_cube_and_ramp() {
        // Exact cube entries must come back unchanged.
        assert_eq!(rgb_to_ansi256(0, 0, 0), 16);
        assert_eq!(rgb_to_ansi256(255, 255, 255), 231);
        assert_eq!(rgb_to_ansi256(255, 0, 0), 196);
        assert_eq!(rgb_to_ansi256(95, 135, 175), 67);
        // Exact ramp entries beat their nearest cube neighbour.
        assert_eq!(rgb_to_ansi256(8, 8, 8), 232);
        assert_eq!(rgb_to_ansi256(238, 238, 238), 255);
        // An off-grid colour lands on the nearest cube entry.
        assert_eq!(rgb_to_ansi256(255, 100, 50), 203);
        // The 16 base entries are never a target.
        for channel in 0..=255u8 {
            assert!(rgb_to_ansi256(channel, channel, channel) >= 16);
        }
    }

    // [spec:mw:req:ansi.adapter.color-depth-inert+1/test]
    #[test]
    fn palette_and_truecolor_quantise_onto_named_entries() {
        // Named entries reproduce themselves exactly.
        assert_eq!(rgb_to_named(0, 0, 0), 0);
        assert_eq!(rgb_to_named(255, 0, 0), 9);
        assert_eq!(rgb_to_named(255, 255, 255), 15);
        assert_eq!(rgb_to_named(255, 100, 50), 9);
        // Indices below 16 already name an entry.
        for n in 0..16u8 {
            assert_eq!(ansi256_to_named(n), n);
        }
        // Cube and ramp boundaries collapse onto their nearest name.
        assert_eq!(ansi256_to_named(16), 0);
        assert_eq!(ansi256_to_named(231), 15);
        assert_eq!(ansi256_to_named(232), 0);
        assert_eq!(ansi256_to_named(255), 7);
        // The dark greys the adapter bands code blocks with.
        assert_eq!(ansi256_to_named(236), 0);
        assert_eq!(ansi256_to_named(237), 0);
        // Every index must resolve to a real named entry.
        for n in 0..=255u8 {
            assert!(ansi256_to_named(n) < 16);
        }
    }

    // [spec:mw:syn:ansi.term.ansi-encoding/test]
    #[test]
    fn color_to_sgr_recovers_short_forms() {
        assert_eq!(color_to_sgr(Color::Named(1), false), "31");
        assert_eq!(color_to_sgr(Color::Named(1), true), "41");
        assert_eq!(color_to_sgr(Color::Named(9), false), "91");
        assert_eq!(color_to_sgr(Color::Named(9), true), "101");
        assert_eq!(color_to_sgr(Color::Palette(236), true), "48;5;236");
        assert_eq!(color_to_sgr(Color::Rgb(1, 2, 3), false), "38;2;1;2;3");
    }
}
