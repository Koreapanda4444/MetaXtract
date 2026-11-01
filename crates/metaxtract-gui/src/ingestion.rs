use crate::state::Desktop;
use eframe::egui;
use std::{collections::BTreeSet, path::PathBuf};

pub fn prepare(
    paths: Vec<PathBuf>,
    existing: impl Iterator<Item = PathBuf>,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut seen: BTreeSet<_> = existing.filter_map(|p| p.canonicalize().ok()).collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for path in paths.into_iter().take(1024) {
        let result = (|| {
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                return Err(std::io::Error::other(crate::i18n::tr(
                    "Choose a regular file or folder",
                )));
            }
            path.canonicalize()
        })();
        match result {
            Ok(path) if seen.contains(&path) => rejected.push(format!(
                "{}: {}",
                crate::i18n::tr("Already queued"),
                path.display()
            )),
            Ok(path) if accepted.len() < 64 => {
                seen.insert(path.clone());
                accepted.push(path);
            }
            Ok(path) => rejected.push(format!(
                "{}: {}",
                crate::i18n::tr("Queue limit reached"),
                path.display()
            )),
            Err(error) => rejected.push(format!("{}: {error}", path.display())),
        }
    }
    (accepted, rejected)
}
impl Desktop {
    pub fn ingest_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.busy || self.readonly {
            self.report_error(crate::i18n::tr(
                "Wait for the current operation or open a writable session.",
            ));
            return;
        }
        let existing = self
            .scan
            .queue
            .iter()
            .cloned()
            .chain(self.scan.active_source.clone());
        let (accepted, rejected) = prepare(paths, existing);
        if !rejected.is_empty() {
            self.notifications.push(
                crate::i18n::tr("Some inputs were skipped"),
                rejected.join("\n"),
                true,
            );
        }
        if accepted.is_empty() {
            return;
        }
        if self.case.is_none() {
            self.quick_analysis_many(ctx, accepted);
        } else {
            let capacity = 64usize.saturating_sub(self.scan.queue.len());
            if accepted.len() > capacity {
                self.report_error(crate::i18n::tr("Queue limit reached"));
            }
            self.scan.queue.extend(accepted.into_iter().take(capacity));
            self.scan.held = false;
        }
    }
    pub fn dropped_files(&mut self, ctx: &egui::Context) {
        let (paths, hovered) = ctx.input(|input| {
            (
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect::<Vec<_>>(),
                !input.raw.hovered_files.is_empty(),
            )
        });
        if !paths.is_empty() {
            self.ingest_paths(ctx, paths);
            self.tab = crate::state::Tab::Scan;
        }
        if hovered {
            egui::Area::new(egui::Id::new("drop-target"))
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.heading(crate::i18n::tr("Drop files or folders to analyze"));
                    });
                });
        }
    }
}
