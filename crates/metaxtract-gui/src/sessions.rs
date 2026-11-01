use crate::{
    files::{Picker, Purpose},
    state::Desktop,
};
use eframe::egui;
use std::path::PathBuf;

#[derive(Default)]
pub struct SessionUi {
    pub confirm_close: bool,
    pub discard_once: bool,
    pub pending: Option<(Purpose, PathBuf)>,
}
impl Desktop {
    pub fn session_notice(&mut self, ctx: &egui::Context) {
        if self.temporary_session.is_some() {
            egui::TopBottomPanel::top("temporary-session").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr(
                        "Temporary analysis. Save as a case to keep these results.",
                    ));
                    if ui
                        .add_enabled(
                            self.can_switch_case(),
                            crate::components::button(crate::i18n::tr("Save as case")),
                        )
                        .clicked()
                    {
                        self.picker = Some(Picker::new(Purpose::SaveSession));
                    }
                });
            });
        }
        if self.session_ui.confirm_close || self.session_ui.pending.is_some() {
            let mut discard = false;
            let mut cancel = false;
            crate::components::modal(crate::i18n::tr("Unsaved analysis")).show(ctx, |ui| {
                ui.label(crate::i18n::tr("This temporary analysis has not been saved. Save it as a case, or discard it to continue."));
                if ui.button(crate::i18n::tr("Save as case")).clicked() {
                    self.picker = Some(Picker::new(Purpose::SaveSession));
                    self.session_ui.confirm_close = false;
                    self.session_ui.pending = None;
                }
                discard = ui.button(crate::i18n::tr("Discard and continue")).clicked();
                cancel = ui.button(crate::i18n::tr("Keep working")).clicked();
            });
            if cancel {
                self.session_ui = SessionUi::default();
                self.scan.close_when_idle = false;
            }
            if discard {
                self.session_ui.discard_once = true;
                if let Some((purpose, path)) = self.session_ui.pending.take() {
                    self.chosen(ctx, purpose, path);
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                self.session_ui.confirm_close = false;
            }
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.case = None;
        self.temporary_session.take();
    }
}
