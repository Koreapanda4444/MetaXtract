use eframe::egui;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub struct Notice {
    pub title: String,
    pub details: String,
    pub error: bool,
    created: Instant,
}
#[derive(Default)]
pub struct Notifications {
    history: VecDeque<Notice>,
    pub open: bool,
}
impl Notifications {
    pub fn push(&mut self, title: impl Into<String>, details: impl Into<String>, error: bool) {
        self.history.push_front(Notice {
            title: title.into(),
            details: details.into(),
            error,
            created: Instant::now(),
        });
        self.history.truncate(100);
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("notifications"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -44.0])
            .show(ctx, |ui| {
                for notice in self.history.iter().take(3).filter(|n| {
                    n.created.elapsed() < Duration::from_secs(if n.error { 12 } else { 5 })
                }) {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.colored_label(
                            if notice.error {
                                egui::Color32::LIGHT_RED
                            } else {
                                ui.visuals().text_color()
                            },
                            &notice.title,
                        );
                        if notice.error && ui.small_button(crate::i18n::tr("Details")).clicked() {
                            self.open = true;
                        }
                    });
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            });
        crate::components::modal(crate::i18n::tr("Notifications")).open(&mut self.open).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
                for notice in &self.history {
                    ui.collapsing(&notice.title, |ui| {
                        ui.add(egui::Label::new(&notice.details).selectable(true).wrap());
                        if notice.error { ui.label(crate::i18n::tr("Check the path and access permissions, then retry. Existing results are preserved.")); }
                        if ui.button(crate::i18n::tr("Copy details")).clicked() { ui.ctx().copy_text(notice.details.clone()); }
                    });
                }
            });
        });
    }
}
