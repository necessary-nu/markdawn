//! Shared SGR → CSS translation for the `ansi` and `terminal` features.
//!
//! Both `ansi_to_html` (byte-stream → HTML) and `terminal` (VT100 grid
//! → HTML) need the same mapping of xterm color palettes and SGR
//! attributes onto inline CSS. Keeping one authoritative copy here
//! means a palette tweak ripples to both pipelines.

/// 4-bit / 8-bit / 24-bit color, matching xterm's SGR 30-37, 90-97,
/// 38;5, 38;2 conventions.
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

/// Translate an xterm 256-color index to a CSS colour. Indices 0–15 are
/// the named palette; 16–231 are a 6x6x6 colour cube; 232–255 are a
/// 24-step greyscale ramp.
pub fn ansi256_css(n: u8) -> String {
    match n {
        0..=15 => named_palette(n).into(),
        16..=231 => {
            let idx = n - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            let c = |v: u8| if v == 0 { 0u8 } else { 55 + v * 40 };
            format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
        }
        232..=255 => {
            let shade = 8 + (n - 232) * 10;
            format!("#{shade:02x}{shade:02x}{shade:02x}")
        }
    }
}

/// Terminal cell style — full SGR attribute set plus fg/bg colors. Both
/// the byte-stream and grid-snapshot pipelines work with this shape.
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
