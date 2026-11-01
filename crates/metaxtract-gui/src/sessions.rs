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
    pub pending: Option<(Purpose, Vec<PathBuf>)>,
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
                    self.chosen_many(ctx, purpose, path);
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
        if let Some(directory) = self.temporary_session.take() {
            self.settings.pending.retain(|pending| {
                pending
                    .case
                    .to_path()
                    .is_ok_and(|p| !p.starts_with(directory.path()))
            });
            let _ = self.settings.save();
            drop(directory);
        }
    }
}

pub fn copy_case(
    case: &metaxtract_app::storage::CaseWorkspace,
    path: &std::path::Path,
) -> std::io::Result<std::sync::Arc<metaxtract_app::storage::CaseWorkspace>> {
    let temporary = tempfile::tempdir()?;
    let bundle = temporary.path().join("copy.zip");
    case.export_snapshot(&bundle, None, &Default::default())?;
    let mut original_journal = case.path().as_os_str().to_os_string();
    original_journal.push(".scan.sqlite");
    let mut saved_journal = path.as_os_str().to_os_string();
    saved_journal.push(".scan.sqlite");
    let original_journal = PathBuf::from(original_journal);
    let saved_journal = PathBuf::from(saved_journal);
    let mut journal_created = false;
    if original_journal.is_file() {
        let mut source = std::fs::File::open(&original_journal)?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&saved_journal)?;
        if let Err(error) = std::io::copy(&mut source, &mut target).and_then(|_| target.sync_all())
        {
            drop(target);
            let _ = std::fs::remove_file(&saved_journal);
            return Err(error);
        }
        journal_created = true;
    }
    let copied = metaxtract_app::storage::CaseWorkspace::open_snapshot(
        bundle,
        path,
        metaxtract_app::storage::AccessMode::ReadWrite,
        None,
        &Default::default(),
    );
    let copied = match copied {
        Ok(case) => std::sync::Arc::new(case),
        Err(error) => {
            if journal_created {
                let _ = std::fs::remove_file(&saved_journal);
            }
            return Err(error);
        }
    };
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saving_a_session_preserves_notes_integrity_and_checkpoint_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("temporary.mxcase");
        let source =
            metaxtract_app::storage::CaseWorkspace::create(&source_path, "Quick analysis").unwrap();
        source.set_notes("preserved notes").unwrap();
        let journal = directory.path().join("temporary.mxcase.scan.sqlite");
        std::fs::write(&journal, b"checkpoint bytes").unwrap();
        let saved_path = directory.path().join("saved.mxcase");
        let saved = copy_case(&source, &saved_path).unwrap();
        assert_eq!(saved.info().unwrap().notes, "preserved notes");
        saved.verify_integrity(None).unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("saved.mxcase.scan.sqlite")).unwrap(),
            b"checkpoint bytes"
        );
        assert!(copy_case(&source, &saved_path).is_err());
        assert_eq!(source.info().unwrap().notes, "preserved notes");
        assert_eq!(std::fs::read(journal).unwrap(), b"checkpoint bytes");
    }
}
