use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app as app;
use std::path::PathBuf;
#[derive(Default)]
pub struct PreviewUi {
    pub record_id: Option<i64>,
    pub content: Option<app::preview::Preview>,
    pub texture: Option<egui::TextureHandle>,
    pub hex: bool,
    pub source_error: Option<String>,
    pub verified_path: Option<PathBuf>,
}
impl Desktop {
    pub fn load_preview(&mut self, ctx: &egui::Context, path: PathBuf) {
        let Some((id, record)) = self.records.focused.clone() else {
            return;
        };
        let chosen_path = path.clone();
        self.task(
            ctx,
            crate::i18n::tr("Verifying and decoding preview"),
            move || app::preview::load(&record, &path),
            move |app, content| {
                if app.records.focused.as_ref().map(|r| r.0) != Some(id) {
                    return;
                }
                app.preview.source_error = None;
                app.preview.verified_path = Some(chosen_path);
                app.preview.record_id = Some(id);
                app.preview.texture = None;
                app.preview.content = Some(content);
            },
        );
    }
    pub fn load_linked_preview(&mut self, ctx: &egui::Context) {
        let (Some(case), Some(scan), Some((id, record))) = (
            self.case.clone(),
            self.active_scan.clone(),
            self.records.focused.clone(),
        ) else {
            return;
        };
        let hex = self.preview.hex;
        self.preview = PreviewUi {
            record_id: Some(id),
            hex,
            ..Default::default()
        };
        self.task(
            ctx,
            crate::i18n::tr("Verifying source and loading preview"),
            move || {
                let result =
                    app::preview::source_candidate(&case, &scan, &record).and_then(|path| {
                        app::preview::load(&record, &path).map(|preview| (path, preview))
                    });
                Ok(result)
            },
            move |app, result| {
                if app.records.focused.as_ref().map(|r| r.0) != Some(id) {
                    return;
                }
                match result {
                    Ok((path, preview)) => {
                        app.preview.verified_path = Some(path);
                        app.preview.content = Some(preview);
                    }
                    Err(error) => app.preview.source_error = Some(error.to_string()),
                }
            },
        );
    }
    pub fn preview_view(&mut self, ui: &mut egui::Ui) {
        let Some((id, _)) = &self.records.focused else {
            ui.label(crate::i18n::tr("Open a record to preview it."));
            return;
        };
        if self.preview.record_id != Some(*id) {
            self.preview = Default::default();
            self.preview.record_id = Some(*id);
        }
        if let Some(path) = &self.preview.verified_path {
            ui.label(format!(
                "{}: {}",
                crate::i18n::tr("Verified source"),
                path.display()
            ));
        }
        if let Some(error) = &self.preview.source_error {
            ui.colored_label(egui::Color32::YELLOW, crate::i18n::tr("Original source is unavailable or changed. Choose a matching source to reconnect."));
            ui.collapsing(crate::i18n::tr("Details"), |ui| {
                ui.add(egui::Label::new(error).selectable(true).wrap());
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    crate::components::button(crate::i18n::tr("Choose matching source")),
                )
                .clicked()
            {
                self.picker = Some(crate::files::Picker::new(crate::files::Purpose::Preview));
            }
            ui.checkbox(&mut self.preview.hex, crate::i18n::tr("Hex"));
            ui.label(crate::i18n::tr(
                "64 MiB source · 64 KiB hex · 32 KiB text · first frame",
            ));
        });
        if let Some(content) = &self.preview.content {
            for note in &content.notices {
                ui.small(crate::i18n::bilingual(note));
            }
            if self.preview.texture.is_none() && !content.rgb.is_empty() {
                self.preview.texture = Some(ui.ctx().load_texture(
                    format!("preview-{id}"),
                    egui::ColorImage::from_rgb([content.width, content.height], &content.rgb),
                    egui::TextureOptions::LINEAR,
                ));
            }
            egui::ScrollArea::both()
                .id_salt("record-preview")
                .max_height(260.0)
                .show(ui, |ui| {
                    if self.preview.hex {
                        egui::ScrollArea::vertical().id_salt("hex-lines").show_rows(
                            ui,
                            18.0,
                            content.hex.len(),
                            |ui, range| {
                                for line in &content.hex[range] {
                                    ui.monospace(line);
                                }
                            },
                        );
                    } else {
                        if let Some(texture) = &self.preview.texture {
                            ui.add(
                                egui::Image::new(texture).fit_to_exact_size(
                                    egui::vec2(content.width as f32, content.height as f32)
                                        * (720.0 / content.width as f32)
                                            .min(400.0 / content.height as f32)
                                            .min(1.0),
                                ),
                            );
                        }
                        if !content.text.is_empty() {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&content.text).monospace())
                                    .selectable(true)
                                    .wrap(),
                            );
                        }
                        if content.text.is_empty() && self.preview.texture.is_none() {
                            ui.label(crate::i18n::tr(
                                "Text/image decoder unavailable. Switch to Hex.",
                            ));
                        }
                    }
                });
        } else {
            ui.label(crate::i18n::tr("Select the original or an independently extracted member. Size and SHA-256 must match."));
        }
    }
}
