use objc2::rc::Retained;
use objc2_ui_kit::UIColor;

pub struct TextKitStyle {
    pub body_font_size: f64,
    pub heading_scale: [f64; 6],
    pub use_dynamic_type: bool,
    pub body_font_name: Option<String>,
    pub code_font_name: Option<String>,

    pub text_color: Retained<UIColor>,
    pub code_background_color: Retained<UIColor>,
    pub code_text_color: Retained<UIColor>,
    pub blockquote_accent_color: Retained<UIColor>,
    pub link_color: Retained<UIColor>,

    pub paragraph_spacing: f64,
    pub list_indent: f64,
    pub blockquote_indent: f64,
    pub heading_spacing_before: [f64; 6],
    pub heading_spacing_after: [f64; 6],

    pub tight_list_spacing: f64,
    pub table_column_padding: f64,

    pub provisional_alpha: f32,
}

impl Default for TextKitStyle {
    fn default() -> Self {
        Self {
            body_font_size: 17.0,
            heading_scale: [2.0, 1.5, 1.17, 1.0, 0.83, 0.75],
            use_dynamic_type: true,
            body_font_name: None,
            code_font_name: None,

            text_color: UIColor::labelColor(),
            code_background_color: UIColor::secondarySystemFillColor(),
            code_text_color: UIColor::labelColor(),
            blockquote_accent_color: UIColor::separatorColor(),
            link_color: UIColor::linkColor(),

            paragraph_spacing: 8.0,
            list_indent: 20.0,
            blockquote_indent: 16.0,
            heading_spacing_before: [24.0, 20.0, 16.0, 12.0, 10.0, 8.0],
            heading_spacing_after: [12.0, 10.0, 8.0, 6.0, 4.0, 4.0],

            tight_list_spacing: 2.0,
            table_column_padding: 12.0,

            provisional_alpha: 0.4,
        }
    }
}
