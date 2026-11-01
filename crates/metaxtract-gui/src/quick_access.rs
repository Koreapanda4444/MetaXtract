use crate::{files::Purpose, state::Desktop};
use eframe::egui;
use metaxtract_app::core::scan::NativePath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Favorite {
    pub path: NativePath,
    pub case: bool,
}
impl Desktop {
    pub fn remember_inputs(&mut self, paths: &[PathBuf]) {
        for path in paths.iter().rev() {
            let native = NativePath::from_path(path);
            if !self.settings.authorized_sources.contains(&native) {
                self.settings.authorized_sources.push(native.clone());
            }
            self.settings.recent_inputs.retain(|p| p != &native);
            self.settings.recent_inputs.insert(0, native);
        }
        self.settings.recent_inputs.truncate(24);
        if self.settings.authorized_sources.len() > 128 {
            self.settings
                .authorized_sources
                .drain(..self.settings.authorized_sources.len() - 128);
        }
        if let Err(error) = self.settings.save() {
            self.report_error(error);
        }
    }
    pub fn quick_access_view(&mut self, ui: &mut egui::Ui) {
        let mut activate = None;
        let mut toggle = None;
        let mut row = |ui: &mut egui::Ui, native: &NativePath, case: bool| {
            if let Ok(path) = native.to_path() {
                let favorite = Favorite {
                    path: native.clone(),
                    case,
                };
                let pinned = self.settings.favorites.contains(&favorite);
                ui.horizontal(|ui| {
                    if ui
                        .small_button(if pinned { "★" } else { "☆" })
                        .on_hover_text(crate::i18n::tr("Toggle favorite"))
                        .clicked()
                    {
                        toggle = Some(favorite);
                    }
                    let name = path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy();
                    if ui
                        .add(egui::Button::new(name.as_ref()).truncate())
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        activate = Some((path, case));
                    }
                });
            }
        };
        ui.separator();
        ui.collapsing(crate::i18n::tr("Favorites"), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("favorites")
                .max_height(150.0)
                .show(ui, |ui| {
                    for favorite in &self.settings.favorites {
                        row(ui, &favorite.path, favorite.case);
                    }
                });
        });
        ui.collapsing(crate::i18n::tr("Recent files and folders"), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("recent-inputs")
                .max_height(180.0)
                .show(ui, |ui| {
                    for path in &self.settings.recent_inputs {
                        row(ui, path, false);
                    }
                });
        });
        ui.collapsing(crate::i18n::tr("Recent cases"), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("recent-cases")
                .max_height(130.0)
                .show(ui, |ui| {
                    for path in &self.settings.recent {
                        row(ui, path, true);
                    }
                });
        });
        if let Some(favorite) = toggle {
            if let Some(i) = self.settings.favorites.iter().position(|p| p == &favorite) {
                self.settings.favorites.remove(i);
            } else if self.settings.favorites.len() < 32 {
                self.settings.favorites.push(favorite);
            }
            if let Err(error) = self.settings.save() {
                self.report_error(error);
            }
        }
        if let Some((path, case)) = activate {
            if case {
                self.chosen(ui.ctx(), Purpose::Open, path);
            } else {
                self.ingest_paths(ui.ctx(), vec![path]);
                self.tab = crate::state::Tab::Scan;
            }
        }
    }
}
