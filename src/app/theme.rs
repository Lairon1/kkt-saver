use eframe::egui::{self, Color32, FontFamily, FontId, Stroke};

pub const EDITOR: Color32 = Color32::from_rgb(30, 31, 34);
pub const PANEL: Color32 = Color32::from_rgb(43, 45, 48);
pub const HOVER: Color32 = Color32::from_rgb(57, 59, 64);
pub const BORDER: Color32 = Color32::from_rgb(67, 69, 74);
pub const TEXT: Color32 = Color32::from_rgb(223, 225, 229);
pub const MUTED: Color32 = Color32::from_rgb(157, 160, 168);
pub const BLUE: Color32 = Color32::from_rgb(53, 116, 240);
pub const SELECTED: Color32 = Color32::from_rgb(46, 67, 110);
pub const GREEN: Color32 = Color32::from_rgb(115, 189, 121);
pub const AMBER: Color32 = Color32::from_rgb(230, 181, 103);
pub const RED: Color32 = Color32::from_rgb(242, 139, 130);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "JetBrains Mono".into(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "JetBrains Mono".into());
    let windows = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"C:\Windows".into());
    if let Ok(bytes) = std::fs::read(windows.join("Fonts/segoeui.ttf")) {
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "Segoe UI".into());
    }
    ctx.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = EDITOR;
    visuals.text_edit_bg_color = Some(EDITOR);
    visuals.faint_bg_color = Color32::from_rgb(35, 37, 40);
    visuals.code_bg_color = EDITOR;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_corner_radius = 6.into();
    visuals.menu_corner_radius = 4.into();
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0_f32, BLUE);
    visuals.hyperlink_color = Color32::from_rgb(108, 157, 255);
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = RED;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = 4.into();
        widget.fg_stroke = Stroke::new(1.0_f32, TEXT);
        widget.bg_stroke = Stroke::new(1.0_f32, BORDER);
        widget.expansion = 0.0;
    }
    visuals.widgets.inactive.bg_fill = HOVER;
    visuals.widgets.inactive.weak_bg_fill = PANEL;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(75, 78, 85);
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.active.bg_fill = SELECTED;
    visuals.widgets.active.weak_bg_fill = SELECTED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, BLUE);
    visuals.widgets.open.bg_fill = HOVER;
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(11.0, 6.0);
        style.spacing.interact_size = egui::vec2(28.0, 28.0);
        style.spacing.window_margin = egui::Margin::same(16);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(22.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    });
}
