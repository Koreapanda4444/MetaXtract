use crate::state::{Desktop, Tab};
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
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        crate::components::apply(&cc.egui_ctx);
        let (sender, receiver) = tasks::channel();
        let mut app = Self {
            tab: Tab::Case,
            records: Default::default(),
            preview: Default::default(),
            inspect: Default::default(),
            search: Default::default(),
            analysis: Default::default(),
            workflows: Default::default(),
            scan: Default::default(),
            case: None,
            info: None,
            scans: vec![],
            history_next: None,
            active_scan: None,
            notes: String::new(),
            title: "Investigation / 조사".into(),
            picker: None,
            settings: Settings::load(),
            status: if korean {
                "Ready / 준비됨".into()
            } else {
                "Ready. Korean font not found; install Noto Sans CJK or set METAXTRACT_FONT_PATH."
                    .into()
            },
            busy: false,
            readonly: false,
            sender,
            receiver,
            smoke,
            frames: 0,
        };
        if let Some(path) = open {
            app.open(&cc.egui_ctx, path, false);
        }
        app
    }
    pub fn task<T: Send + 'static>(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        work: impl FnOnce() -> io::Result<T> + Send + 'static,
        apply: impl FnOnce(&mut Desktop, T) + Send + 'static,
    ) {
        if self.busy {
            self.status =
                "Finish the current operation first. / 현재 작업이 끝난 후 실행하세요.".into();
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
                        app.status = "Completed / 완료".into();
                        apply(app, v);
                    }
                    Err(e) => app.status = e.to_string(),
                }));
                context.request_repaint();
            })
        {
            self.busy = false;
            self.status = e.to_string();
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
        self.scan.close_when_idle = closing;
        if let Err(e) = self.settings.remember(case.path()) {
            self.status = format!("Case opened; recent list was not saved: {e}");
        }
        self.readonly = case.access_mode() == storage::AccessMode::ReadOnly;
        self.title = info.title.clone();
        self.notes = info.notes.clone();
        self.active_scan = history.items.first().map(|s| s.id.clone());
        self.case = Some(case);
        self.info = Some(info);
        self.scans = history.items;
        self.history_next = history.next;
    }
    pub fn open(&mut self, ctx: &egui::Context, path: PathBuf, create: bool) {
        let readonly = self.readonly;
        let title = self.title.clone();
        self.task(
            ctx,
            "Opening case / 케이스 열기",
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
                        "Copying case / 케이스 복사",
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
