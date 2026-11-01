use eframe::egui;
use std::{io, path::PathBuf, sync::mpsc};
#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    Open,
    New,
    Copy,
    SaveSession,
    Source,
    Quick,
    Csv,
    Jsonl,
    Preview,
    GeoJson,
    Kml,
    Workflow(crate::workflows::Field),
}
pub struct Picker {
    pub purpose: Purpose,
    result: Option<mpsc::Receiver<io::Result<Option<PathBuf>>>>,
    error: Option<String>,
}
impl Picker {
    pub fn new(purpose: Purpose) -> Self {
        Self {
            purpose,
            result: None,
            error: None,
        }
    }
    fn launch(&mut self, ctx: &egui::Context, folder: bool) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let purpose = self.purpose;
        let title = crate::i18n::tr("Choose path").to_owned();
        let context = ctx.clone();
        let task = std::thread::Builder::new()
            .name("metaxtract-file-dialog".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(move || {
                    let dialog = rfd::FileDialog::new().set_title(title);
                    if folder {
                        dialog.pick_folder()
                    } else {
                        match purpose {
                            Purpose::New | Purpose::Copy | Purpose::SaveSession => dialog
                                .add_filter("MetaXtract", &["mxcase"])
                                .set_file_name("analysis.mxcase")
                                .save_file(),
                            Purpose::Csv => dialog
                                .add_filter("CSV", &["csv"])
                                .set_file_name("metadata.csv")
                                .save_file(),
                            Purpose::Jsonl => dialog
                                .add_filter("JSONL", &["jsonl"])
                                .set_file_name("metadata.jsonl")
                                .save_file(),
                            Purpose::GeoJson => {
                                dialog.add_filter("GeoJSON", &["geojson"]).save_file()
                            }
                            Purpose::Kml => dialog.add_filter("KML", &["kml"]).save_file(),
                            Purpose::Workflow(
                                crate::workflows::Field::Output | crate::workflows::Field::Receipt,
                            ) => dialog.save_file(),
                            _ => dialog.pick_file(),
                        }
                    }
                })
                .map_err(|_| io::Error::other("Native dialog failed"));
                let _ = sender.send(result);
                context.request_repaint();
            });
        match task {
            Ok(_) => self.result = Some(receiver),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<PathBuf>) {
        if let Some(receiver) = &self.result {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            match receiver.try_recv() {
                Ok(Ok(path)) => return (false, path),
                Ok(Err(error)) => {
                    self.error = Some(error.to_string());
                    self.result = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Native dialog disconnected".into());
                    self.result = None;
                }
                Err(mpsc::TryRecvError::Empty) => return (true, None),
            }
        }
        let mut open = true;
        let mut choose = None;
        if matches!(self.purpose, Purpose::Source | Purpose::Quick) || self.error.is_some() {
            crate::components::modal(crate::i18n::tr("Choose path"))
                .open(&mut open)
                .show(ctx, |ui| {
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    if ui.button(crate::i18n::tr("Choose file")).clicked() {
                        choose = Some(false);
                    }
                    if ui.button(crate::i18n::tr("Choose folder")).clicked() {
                        choose = Some(true);
                    }
                });
        } else {
            choose = Some(false);
        }
        if let Some(folder) = choose {
            self.error = None;
            self.launch(ctx, folder);
        }
        (open, None)
    }
}
