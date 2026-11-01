use crate::{
    desktop::Desktop,
    files::{Picker, Purpose},
};
use eframe::egui;
use metaxtract_app::{self as app, core::scan::NativePath};
use std::{io, path::Path};

#[derive(Clone)]
pub enum FileAction {
    CopyPath(String),
    Inspect(i64),
    Reanalyze(Vec<i64>),
    Export(i64, bool),
}
pub fn authorized(path: &Path, roots: &[NativePath]) -> bool {
    roots
        .iter()
        .filter_map(|root| root.to_path().ok())
        .any(|root| path == root || (root.is_dir() && path.starts_with(root)))
}
impl Desktop {
    pub fn file_action(&mut self, ctx: &egui::Context, action: FileAction) {
        match action {
            FileAction::CopyPath(path) => ctx.copy_text(path),
            FileAction::Inspect(id) => {
                self.settings.workspace.show_inspector = true;
                self.focus_record(ctx, id);
            }
            FileAction::Export(id, csv) => {
                if !self.records.selected.contains(&id) {
                    self.records.selected.clear();
                    self.records.selected.insert(id);
                }
                self.picker = Some(Picker::new(if csv { Purpose::Csv } else { Purpose::Jsonl }));
            }
            FileAction::Reanalyze(ids) => {
                let (Some(case), Some(scan)) = (self.case.clone(), self.active_scan.clone()) else {
                    return;
                };
                let roots = self.settings.authorized_sources.clone();
                let context = ctx.clone();
                self.task(
                    ctx,
                    crate::i18n::tr("Resolving original sources"),
                    move || {
                        let mut paths = Vec::new();
                        let mut errors = Vec::new();
                        for id in ids.into_iter().take(64) {
                            let result = (|| {
                                let record = case
                                    .record(id)?
                                    .ok_or_else(|| io::Error::other("record unavailable"))?;
                                let path = app::preview::source_candidate(&case, &scan, &record)?
                                    .canonicalize()?;
                                if !authorized(&path, &roots) {
                                    return Err(io::Error::other(
                                        "source is not authorized on this computer",
                                    ));
                                }
                                Ok(path)
                            })();
                            match result {
                                Ok(path) => paths.push(path),
                                Err(error) => errors.push(format!("{id}: {error}")),
                            }
                        }
                        if paths.is_empty() {
                            return Err(io::Error::other(errors.join("\n")));
                        }
                        Ok((paths, errors))
                    },
                    move |app, (paths, errors)| {
                        if !errors.is_empty() {
                            app.notifications.push(
                                crate::i18n::tr("Some inputs were skipped"),
                                errors.join("\n"),
                                true,
                            );
                        }
                        app.ingest_paths(&context, paths);
                    },
                );
            }
        }
    }
}
