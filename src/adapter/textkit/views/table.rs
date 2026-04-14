use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;
use objc2_ui_kit::{UIColor, UIFont, UILabel, UIView};

use crate::adapter::textkit::attrs;
use crate::adapter::textkit::style::TextKitStyle;

pub fn build_table_view(
    header_cells: &[String],
    rows: &[Vec<String>],
    col_widths: &[usize],
    style: &TextKitStyle,
    available_width: f64,
    mtm: MainThreadMarker,
) -> Retained<UIView> {
    let row_height: f64 = style.body_font_size * 1.8;
    let separator_width: f64 = 1.0 / 3.0; // 1px on 3x displays
    let padding: f64 = 8.0;
    let total_rows = 1 + rows.len(); // header + data
    let total_height = total_rows as f64 * row_height
        + separator_width * (total_rows + 1) as f64;

    let container = UIView::initWithFrame(
        UIView::alloc(mtm),
        CGRect::new(CGPoint::ZERO, CGSize { width: available_width, height: total_height }),
    );
    container.setBackgroundColor(Some(&UIColor::systemBackgroundColor()));

    // Compute column pixel widths proportional to character widths
    let total_chars: usize = col_widths.iter().sum::<usize>().max(1);
    let usable_width = available_width - separator_width * (col_widths.len() + 1) as f64;
    let col_pixel_widths: Vec<f64> = col_widths
        .iter()
        .map(|&w| (w.max(1) as f64 / total_chars as f64) * usable_width)
        .collect();

    let separator_color = UIColor::separatorColor();
    let header_bg = UIColor::secondarySystemBackgroundColor();
    let bold_font = attrs::heading_font(style, 6); // smallest heading = bold body
    let body_font = attrs::body_font(style);

    // Top border
    add_separator(&container, 0.0, 0.0, available_width, separator_width, &separator_color, mtm);

    let mut y = separator_width;

    // Header row
    {
        let row_bg = UIView::initWithFrame(
            UIView::alloc(mtm),
            CGRect::new(CGPoint::new(0.0, y), CGSize { width: available_width, height: row_height }),
        );
        row_bg.setBackgroundColor(Some(&header_bg));
        container.addSubview(&row_bg);

        let mut x = separator_width;
        for (i, text) in header_cells.iter().enumerate() {
            let col_w = col_pixel_widths.get(i).copied().unwrap_or(100.0);
            add_cell_label(&container, x + padding, y, col_w - padding * 2.0, row_height, text, &bold_font, &style.text_color, mtm);
            x += col_w;
            add_separator(&container, x, y, separator_width, row_height, &separator_color, mtm);
            x += separator_width;
        }
        // Left border
        add_separator(&container, 0.0, y, separator_width, row_height, &separator_color, mtm);

        y += row_height;
        // Header bottom border
        add_separator(&container, 0.0, y, available_width, separator_width, &separator_color, mtm);
        y += separator_width;
    }

    // Data rows
    for row in rows {
        let mut x = separator_width;
        for (i, text) in row.iter().enumerate() {
            let col_w = col_pixel_widths.get(i).copied().unwrap_or(100.0);
            add_cell_label(&container, x + padding, y, col_w - padding * 2.0, row_height, text, &body_font, &style.text_color, mtm);
            x += col_w;
            add_separator(&container, x, y, separator_width, row_height, &separator_color, mtm);
            x += separator_width;
        }
        // Left border
        add_separator(&container, 0.0, y, separator_width, row_height, &separator_color, mtm);

        y += row_height;
        // Row bottom border
        add_separator(&container, 0.0, y, available_width, separator_width, &separator_color, mtm);
        y += separator_width;
    }

    container
}

pub fn compute_table_height(
    row_count: usize,
    style: &TextKitStyle,
) -> f64 {
    let row_height = style.body_font_size * 1.8;
    let separator_width = 1.0 / 3.0;
    let total_rows = 1 + row_count;
    total_rows as f64 * row_height + separator_width * (total_rows + 1) as f64
}

fn add_separator(
    parent: &UIView,
    x: f64, y: f64, w: f64, h: f64,
    color: &UIColor,
    mtm: MainThreadMarker,
) {
    let sep = UIView::initWithFrame(
        UIView::alloc(mtm),
        CGRect::new(CGPoint::new(x, y), CGSize { width: w, height: h }),
    );
    sep.setBackgroundColor(Some(color));
    parent.addSubview(&sep);
}

fn add_cell_label(
    parent: &UIView,
    x: f64, y: f64, w: f64, h: f64,
    text: &str,
    font: &UIFont,
    color: &UIColor,
    mtm: MainThreadMarker,
) {
    let label = UILabel::initWithFrame(
        UILabel::alloc(mtm),
        CGRect::new(CGPoint::new(x, y), CGSize { width: w, height: h }),
    );
    let ns_text = NSString::from_str(text);
    label.setText(Some(&ns_text));
    unsafe {
        label.setFont(Some(font));
        label.setTextColor(Some(color));
    }
    parent.addSubview(&label);
}
