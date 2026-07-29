//! Table view factory: Auto Layout nested UIStackView grid.
//!
//! Structure (see plan):
//! - Outer container UIView (background = separatorColor, cornerRadius)
//! - Inner vertical UIStackView pinned with 1pt inset — spacing = 1pt between rows
//!   creates the horizontal grid lines via the container background showing through
//! - Each row is a horizontal UIStackView with fillEqually + spacing = 1pt for
//!   vertical grid lines via background bleed-through
//! - Each cell is a UIView with opaque background containing a UILabel pinned
//!   with 8pt padding

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSMutableAttributedString, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSTextAlignment, UIColor, UIEdgeInsets, UILayoutConstraintAxis,
    UIStackView, UIStackViewAlignment, UIStackViewDistribution, UITextView, UIView,
};

use crate::adapter::uikit::style::UIKitStyle;
use crate::node::TableAlignment;

const CELL_PADDING: f64 = 8.0;
const BORDER: f64 = 1.0;
const CORNER_RADIUS: f64 = 6.0;

/// Handles to every cell text view in a built table, indexed in row-major order.
/// `header` is empty when the table has no header row.
pub struct TableHandles {
    pub header: Vec<Retained<UITextView>>,
    pub rows: Vec<Vec<Retained<UITextView>>>,
}

impl TableHandles {
    /// Returns true when `header` and `rows` share the same shape (column count
    /// per row, plus matching header column count if either has a header).
    pub fn shape_matches(
        &self,
        new_header: &[Retained<NSMutableAttributedString>],
        new_rows: &[Vec<Retained<NSMutableAttributedString>>],
    ) -> bool {
        if self.header.len() != new_header.len() {
            return false;
        }
        if self.rows.len() != new_rows.len() {
            return false;
        }
        for (existing, incoming) in self.rows.iter().zip(new_rows.iter()) {
            if existing.len() != incoming.len() {
                return false;
            }
        }
        true
    }

    /// Replace the attributed text of every cell in place. Caller must have
    /// confirmed `shape_matches` beforehand.
    pub fn update_in_place(
        &self,
        new_header: &[Retained<NSMutableAttributedString>],
        new_rows: &[Vec<Retained<NSMutableAttributedString>>],
    ) {
        for (text_view, attr) in self.header.iter().zip(new_header.iter()) {
            text_view.setAttributedText(Some(attr));
        }
        for (existing_row, incoming_row) in self.rows.iter().zip(new_rows.iter()) {
            for (text_view, attr) in existing_row.iter().zip(incoming_row.iter()) {
                text_view.setAttributedText(Some(attr));
            }
        }
    }
}

// [spec:mw:req:uikit.style.system-colors]
pub fn build_table_view(
    header_cells: &[Retained<NSMutableAttributedString>],
    data_rows: &[Vec<Retained<NSMutableAttributedString>>],
    alignments: &[Option<TableAlignment>],
    _style: &UIKitStyle,
    mtm: MainThreadMarker,
) -> (Retained<UIView>, TableHandles) {
    let separator_color = UIColor::separatorColor();
    let cell_bg = UIColor::systemBackgroundColor();
    let header_bg = UIColor::secondarySystemBackgroundColor();

    // Outer container: its background shows through as separator lines
    let container = UIView::new(mtm);
    container.setTranslatesAutoresizingMaskIntoConstraints(false);
    container.setBackgroundColor(Some(&separator_color));
    container.setClipsToBounds(true);
    container.layer().setCornerRadius(CORNER_RADIUS);

    // Vertical stack of rows
    let v_stack = UIStackView::new(mtm);
    v_stack.setAxis(UILayoutConstraintAxis::Vertical);
    v_stack.setDistribution(UIStackViewDistribution::Fill);
    v_stack.setAlignment(UIStackViewAlignment::Fill);
    v_stack.setSpacing(BORDER);
    v_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    container.addSubview(&v_stack);

    // Pin vertical stack to container with 1pt inset (creates outer border)
    pin_inside(&v_stack, &container, BORDER);

    let mut header_handles: Vec<Retained<UITextView>> = Vec::new();
    if !header_cells.is_empty() {
        let (row, cell_views) = build_row(header_cells, alignments, &header_bg, mtm);
        v_stack.addArrangedSubview(&row);
        header_handles = cell_views;
    }

    let mut row_handles: Vec<Vec<Retained<UITextView>>> = Vec::with_capacity(data_rows.len());
    for cells in data_rows {
        let (row, cell_views) = build_row(cells, alignments, &cell_bg, mtm);
        v_stack.addArrangedSubview(&row);
        row_handles.push(cell_views);
    }

    let cols = header_cells
        .len()
        .max(data_rows.first().map_or(0, |r| r.len()));
    let rows = data_rows.len();
    let a11y = NSString::from_str(&format!("Table, {cols} columns, {rows} rows"));
    container.setAccessibilityLabel(Some(&a11y), mtm);

    (
        container,
        TableHandles {
            header: header_handles,
            rows: row_handles,
        },
    )
}

/// Build a single row as a horizontal UIStackView of cells. Returns the row
/// stack and the inner text view of each cell, in column order.
fn build_row(
    cells: &[Retained<NSMutableAttributedString>],
    alignments: &[Option<TableAlignment>],
    cell_bg: &UIColor,
    mtm: MainThreadMarker,
) -> (Retained<UIStackView>, Vec<Retained<UITextView>>) {
    let h_stack = UIStackView::new(mtm);
    h_stack.setAxis(UILayoutConstraintAxis::Horizontal);
    h_stack.setDistribution(UIStackViewDistribution::FillEqually);
    h_stack.setAlignment(UIStackViewAlignment::Fill);
    h_stack.setSpacing(BORDER);
    h_stack.setTranslatesAutoresizingMaskIntoConstraints(false);

    let mut text_views = Vec::with_capacity(cells.len());
    for (i, cell_text) in cells.iter().enumerate() {
        let align = alignments.get(i).copied().flatten();
        let (cell, text_view) = build_cell(cell_text, align, cell_bg, mtm);
        h_stack.addArrangedSubview(&cell);
        text_views.push(text_view);
    }

    (h_stack, text_views)
}

/// Build a single cell: a UIView with opaque background containing a non-editable
/// UITextView (selectable, stable intrinsic height) pinned with padding. Returns
/// the cell container and the inner text view.
fn build_cell(
    text: &NSMutableAttributedString,
    alignment: Option<TableAlignment>,
    bg: &UIColor,
    mtm: MainThreadMarker,
) -> (Retained<UIView>, Retained<UITextView>) {
    let cell = UIView::new(mtm);
    cell.setBackgroundColor(Some(bg));
    cell.setTranslatesAutoresizingMaskIntoConstraints(false);

    let text_view = UITextView::initWithFrame_textContainer(
        UITextView::alloc(mtm),
        objc2_core_foundation::CGRect::ZERO,
        None,
    );
    text_view.setTranslatesAutoresizingMaskIntoConstraints(false);
    text_view.setEditable(false);
    text_view.setScrollEnabled(false);
    text_view.setBackgroundColor(Some(&UIColor::clearColor()));
    text_view.setTextContainerInset(UIEdgeInsets {
        top: 0.0,
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
    });
    text_view.setAttributedText(Some(text));
    text_view.textContainer().setLineFragmentPadding(0.0);
    if let Some(align) = alignment {
        text_view.setTextAlignment(match align {
            TableAlignment::Left => NSTextAlignment::Left,
            TableAlignment::Center => NSTextAlignment::Center,
            TableAlignment::Right => NSTextAlignment::Right,
        });
    }
    cell.addSubview(&text_view);

    // Pin text view with CELL_PADDING inset on all sides
    pin_inside(&text_view, &cell, CELL_PADDING);

    (cell, text_view)
}

/// Pin `inner` inside `outer` with a uniform inset on all four edges.
fn pin_inside(inner: &UIView, outer: &UIView, inset: f64) {
    let top = inner
        .topAnchor()
        .constraintEqualToAnchor_constant(&outer.topAnchor(), inset);
    let leading = inner
        .leadingAnchor()
        .constraintEqualToAnchor_constant(&outer.leadingAnchor(), inset);
    let trailing = outer
        .trailingAnchor()
        .constraintEqualToAnchor_constant(&inner.trailingAnchor(), inset);
    let bottom = outer
        .bottomAnchor()
        .constraintEqualToAnchor_constant(&inner.bottomAnchor(), inset);
    top.setActive(true);
    leading.setActive(true);
    trailing.setActive(true);
    bottom.setActive(true);
}
