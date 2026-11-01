use crate::state::Desktop;
use eframe::egui;
use metaxtract_app::{self as app, core::scan::NativePath};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingQueue {
    pub case: NativePath,
    pub sources: Vec<NativePath>,
    pub temporary: bool,
}
pub struct TemporarySession {
    directory: PathBuf,
}
impl TemporarySession {
    pub fn new() -> io::Result<Self> {
        let base = crate::settings::session_directory()?;
        std::fs::create_dir_all(&base)?;
        let directory = tempfile::Builder::new()
            .prefix("analysis-")
            .tempdir_in(base)?
            .keep();
        Ok(Self { directory })
    }
    pub fn restore(case: &Path) -> io::Result<Self> {
        let base = crate::settings::session_directory()?.canonicalize()?;
        let directory = case
            .parent()
            .ok_or_else(|| io::Error::other("missing session parent"))?
            .canonicalize()?;
        if directory.parent() != Some(base.as_path())
            || !directory
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("analysis-"))
        {
            return Err(io::Error::other("invalid temporary session recovery path"));
        }
        Ok(Self { directory })
    }
    pub fn path(&self) -> &Path {
        &self.directory
    }
}
impl Drop for TemporarySession {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
impl Desktop {
    pub fn persist_pending_queue(&mut self) {
        let Some(case) = &self.case else {
            return;
        };
        let path = NativePath::from_path(case.path());
        let sources: Vec<_> = self
            .scan
            .queue
            .iter()
            .map(|p| NativePath::from_path(p))
            .collect();
        let mut pending = self.settings.pending.clone();
        pending.retain(|item| item.case != path);
        if !sources.is_empty() {
            pending.push(PendingQueue {
                case: path,
                sources,
                temporary: self.temporary_session.is_some(),
            });
        }
        pending.truncate(32);
        if pending != self.settings.pending {
            self.settings.pending = pending;
            if let Err(error) = self.settings.save() {
                self.scan.held = true;
                self.report_error(error);
            }
        }
    }
    pub fn restore_pending_queue(&mut self) {
        let Some(case) = &self.case else {
            return;
        };
        if let Some(pending) = self
            .settings
            .pending
            .iter()
            .find(|p| p.case == NativePath::from_path(case.path()))
        {
            let sources = pending
                .sources
                .iter()
                .filter_map(|p| p.to_path().ok())
                .collect();
            let (accepted, rejected) = crate::ingestion::prepare(sources, std::iter::empty());
            self.scan.queue.extend(accepted);
            self.scan.held = true;
            self.notifications.push(
                crate::i18n::tr("Pending scans restored. Continue the queue when ready."),
                rejected.join("\n"),
                !rejected.is_empty(),
            );
        }
    }
    pub fn recovery_view(&mut self, ui: &mut egui::Ui) {
        if self.settings.pending.is_empty() {
            return;
        }
        ui.heading(crate::i18n::tr("Recover pending scans"));
        let mut restore = None;
        let mut forget = None;
        for (i, pending) in self.settings.pending.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if let Ok(path) = pending.case.to_path() {
                    if ui
                        .button(format!("{} · {}", path.display(), pending.sources.len()))
                        .clicked()
                    {
                        restore = Some(i);
                    }
                }
                if ui.small_button(crate::i18n::tr("Forget queue")).clicked() {
                    forget = Some(i);
                }
            });
        }
        if let Some(i) = forget {
            self.settings.pending.remove(i);
            if let Err(error) = self.settings.save() {
                self.report_error(error);
            }
        }
        if let Some(i) = restore {
            let pending = self.settings.pending[i].clone();
            self.task(
                ui.ctx(),
                crate::i18n::tr("Restoring queued analysis"),
                move || {
                    let path = pending.case.to_path()?;
                    let case = std::sync::Arc::new(app::open_case(&path, true)?);
                    let temporary = if pending.temporary {
                        Some(TemporarySession::restore(&path)?)
                    } else {
                        None
                    };
                    let info = case.info()?;
                    let history = case.scan_history(None, 128)?;
                    Ok((case, temporary, info, history))
                },
                |app, (case, temporary, info, history)| {
                    app.temporary_session = temporary;
                    app.loaded(case, info, history);
                    app.tab = crate::state::Tab::Scan;
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_recovery_paths_never_acquire_a_cleanup_guard() {
        let directory = tempfile::tempdir().unwrap();
        let case = directory.path().join("case.mxcase");
        std::fs::write(&case, b"preserve").unwrap();
        assert!(TemporarySession::restore(&case).is_err());
        assert_eq!(std::fs::read(case).unwrap(), b"preserve");
    }
}
