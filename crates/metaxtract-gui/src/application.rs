use crate::state::Desktop;
use crate::tasks;
use crate::{
    files::{Picker, Purpose},
    settings::Settings,
};
use eframe::egui;
use metaxtract_app::{
    self as app,
    storage::{self, CaseInfo, CaseWorkspace},
};
use std::{io, path::PathBuf, sync::Arc};
impl Desktop {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        open: Option<PathBuf>,
        smoke: bool,
        korean: bool,
    ) -> Self {
        let settings = Settings::load();
        crate::i18n::set(
            settings
                .language
                .unwrap_or_else(crate::i18n::system_language),
        );
        settings.appearance.apply(&cc.egui_ctx, settings.scale);
        let (sender, receiver) = tasks::channel();
        let mut app = Self {
            tab: settings.workspace.tab,
            records: Default::default(),
            preview: Default::default(),
            inspect: Default::default(),
            search: Default::default(),
            analysis: Default::default(),
            workflows: Default::default(),
            scan: Default::default(),
            case: None,
            temporary_session: None,
            info: None,
            scans: vec![],
            history_next: None,
            active_scan: None,
            notes: String::new(),
            title: crate::i18n::tr("Investigation").into(),
            picker: None,
            saved_settings: serde_json::to_string(&settings).unwrap_or_default(),
            settings_checkpoint: std::time::Instant::now(),
            settings,
            status: if korean {
                crate::i18n::tr("Ready").into()
            } else {
                "Ready. Korean font not found; install Noto Sans CJK or set METAXTRACT_FONT_PATH."
                    .into()
            },
            busy: false,
            notifications: Default::default(),
            readonly: false,
            sender,
            receiver,
            smoke,
            frames: 0,
        };
        app.restore_display_preferences();
        if let Some(path) = open {
            app.open(&cc.egui_ctx, path, false);
        }
        app
    }
    fn restore_display_preferences(&mut self) {
        let w = &self.settings.workspace;
        self.records.order = w.order;
        self.preview.hex = w.preview_hex;
        self.scan.include_hidden = w.include_hidden;
        self.scan.decode_previews = w.decode_previews;
        self.scan.workers = w.workers.clamp(1, 32);
    }
    pub fn save_display_preferences(&mut self, ctx: &egui::Context, force: bool) {
        if !force && self.settings_checkpoint.elapsed().as_secs() < 2 {
            return;
        }
        self.settings_checkpoint = std::time::Instant::now();
        let w = &mut self.settings.workspace;
        w.tab = self.tab;
        w.order = self.records.order;
        w.preview_hex = self.preview.hex;
        w.include_hidden = self.scan.include_hidden;
        w.decode_previews = self.scan.decode_previews;
        w.workers = self.scan.workers;
        ctx.input(|input| {
            let viewport = input.viewport();
            if !viewport.maximized.unwrap_or(false) && !viewport.minimized.unwrap_or(false) {
                if let Some(rect) = viewport.inner_rect {
                    w.window_size = [rect.width(), rect.height()];
                }
                if let Some(rect) = viewport.outer_rect {
                    w.window_position = Some([rect.min.x, rect.min.y]);
                }
            }
        });
        let current = serde_json::to_string(&self.settings).unwrap_or_default();
        if current != self.saved_settings {
            match self.settings.save() {
                Ok(()) => self.saved_settings = current,
                Err(error) => self.report_error(error),
            }
        }
    }
    pub fn can_switch_case(&self) -> bool {
        !self.busy
            && !self.scan.running()
            && self.scan.queue.is_empty()
            && !self
                .info
                .as_ref()
                .is_some_and(|i| i.notes != self.notes || i.title != self.title)
    }
    pub fn quick_analysis(&mut self, ctx: &egui::Context, path: PathBuf) {
        if !self.can_switch_case() {
            self.report_error(crate::i18n::tr(
                "Finish or save the current work before opening another session.",
            ));
            return;
        }
        self.task(
            ctx,
            crate::i18n::tr("Preparing quick analysis"),
            move || {
                let directory = tempfile::Builder::new()
                    .prefix("metaxtract-session-")
                    .tempdir()?;
                let case = Arc::new(CaseWorkspace::create(
                    directory.path().join("session.mxcase"),
                    crate::i18n::tr("Quick analysis"),
                )?);
                let info = case.info()?;
                let history = case.scan_history(None, 128)?;
                Ok((directory, case, info, history))
            },
            move |app, (directory, case, info, history)| {
                app.case = None;
                app.temporary_session = Some(directory);
                app.loaded(case, info, history);
                app.scan.queue.push_back(path);
                app.tab = crate::state::Tab::Scan;
            },
        );
    }
    pub fn report_error(&mut self, error: impl std::fmt::Display) {
        self.status = error.to_string();
        self.notifications.push(
            crate::i18n::tr("Operation failed"),
            self.status.clone(),
            true,
        );
    }
    pub fn task<T: Send + 'static>(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        work: impl FnOnce() -> io::Result<T> + Send + 'static,
        apply: impl FnOnce(&mut Desktop, T) + Send + 'static,
    ) {
        if self.busy {
            self.status = crate::i18n::tr("Finish the current operation first.").into();
            return;
        }
        self.busy = true;
        self.status = label.into();
        let sender = self.sender.clone();
        let context = ctx.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("metaxtract-desktop".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err(io::Error::other("background operation panicked")));
                let _ = sender.send(Box::new(move |app| match result {
                    Ok(v) => {
                        app.status = crate::i18n::tr("Completed").into();
                        apply(app, v);
                        app.notifications
                            .push(app.status.clone(), String::new(), false);
                    }
                    Err(e) => app.report_error(e),
                }));
                context.request_repaint();
            })
        {
            self.busy = false;
            self.report_error(e);
        }
    }
    pub fn loaded(
        &mut self,
        case: Arc<CaseWorkspace>,
        info: CaseInfo,
        history: storage::ScanHistoryPage,
    ) {
        let closing = self.scan.close_when_idle;
        self.scan = Default::default();
        self.records = Default::default();
        self.preview = Default::default();
        self.inspect = Default::default();
        self.search = Default::default();
        self.analysis = Default::default();
        self.workflows = Default::default();
        self.restore_display_preferences();
        self.scan.close_when_idle = closing;
        let temporary = self
            .temporary_session
            .as_ref()
            .is_some_and(|d| case.path().starts_with(d.path()));
        if !temporary {
            if let Err(e) = self.settings.remember(case.path()) {
                self.status = format!("Case opened; recent list was not saved: {e}");
            }
        }
        self.readonly = case.access_mode() == storage::AccessMode::ReadOnly;
        self.title = info.title.clone();
        self.notes = info.notes.clone();
        self.active_scan = history.items.first().map(|s| s.id.clone());
        self.case = Some(case);
        if !temporary {
            self.temporary_session = None;
        }
        self.info = Some(info);
        self.scans = history.items;
        self.history_next = history.next;
    }
    pub fn open(&mut self, ctx: &egui::Context, path: PathBuf, create: bool) {
        let readonly = self.readonly;
        let title = self.title.clone();
        self.task(
            ctx,
            crate::i18n::tr("Opening case"),
            move || {
                let case = Arc::new(if create {
                    CaseWorkspace::create(&path, &title)?
                } else {
                    app::open_case(&path, !readonly)?
                });
                let info = case.info()?;
                let scans = case.scan_history(None, 128)?;
                Ok((case, info, scans))
            },
            |app, (case, info, scans)| app.loaded(case, info, scans),
        );
    }
    pub(crate) fn chosen(&mut self, ctx: &egui::Context, purpose: Purpose, path: PathBuf) {
        match purpose {
            Purpose::Workflow(field) => self.workflow_path(field, path),
            Purpose::GeoJson => self.export_geo(ctx, path, false),
            Purpose::Kml => self.export_geo(ctx, path, true),
            Purpose::Preview => self.load_preview(ctx, path),
            Purpose::Csv => self.export_selection(ctx, path, true),
            Purpose::Jsonl => self.export_selection(ctx, path, false),
            Purpose::Quick => self.quick_analysis(ctx, path),
            Purpose::Source => {
                self.scan.source = path.to_string_lossy().into_owned();
                self.scan.source_path = Some(path);
            }
            Purpose::Open => self.open(ctx, path, false),
            Purpose::New => self.open(ctx, path, true),
            Purpose::Copy => {
                if let Some(case) = self.case.clone() {
                    self.task(
                        ctx,
                        crate::i18n::tr("Copying case"),
                        move || {
                            let temporary = tempfile::tempdir()?;
                            let bundle = temporary.path().join("copy.zip");
                            case.export_snapshot(&bundle, None, &Default::default())?;
                            let copied = Arc::new(CaseWorkspace::open_snapshot(
                                bundle,
                                path,
                                storage::AccessMode::ReadWrite,
                                None,
                                &Default::default(),
                            )?);
                            let info = copied.info()?;
                            let scans = copied.scan_history(None, 128)?;
                            Ok((copied, info, scans))
                        },
                        |app, (case, info, scans)| app.loaded(case, info, scans),
                    );
                }
            }
        }
    }
}
