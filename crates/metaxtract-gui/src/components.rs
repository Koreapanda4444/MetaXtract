use eframe::egui;

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.visuals.panel_fill = if style.visuals.dark_mode {
        egui::Color32::from_rgb(23, 28, 36)
    } else {
        egui::Color32::from_rgb(245, 247, 250)
    };
    ctx.set_style(style);
}

pub fn button(label: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(label).min_size(egui::vec2(0.0, 30.0))
}

pub fn heading(ui: &mut egui::Ui, title: impl Into<egui::RichText>) {
    ui.add_space(4.0);
    ui.heading(title);
    ui.add_space(4.0);
}

pub fn path_input(ui: &mut egui::Ui, value: &mut String) -> egui::Response {
    ui.add(egui::TextEdit::singleline(value).desired_width(420.0))
}

pub fn status(ui: &mut egui::Ui, text: &str, busy: bool) {
    ui.horizontal_wrapped(|ui| {
        if busy {
            ui.spinner();
        }
        ui.label(crate::i18n::bilingual(text));
    });
}

pub fn modal(title: &str) -> egui::Window<'_> {
    egui::Window::new(title).collapsible(false).resizable(true)
}
