use crate::{
    files::{Picker, Purpose},
    settings::Settings,
};
use eframe::egui;
use metaxtract_app::{
    self as app,
    storage::{self, CaseInfo, CaseWorkspace, ScanInfo},
};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, mpsc},
};
type Callback = Box<dyn FnOnce(&mut Desktop) + Send>;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Case,
    Scan,
    Records,
}
pub struct Desktop {
    pub tab: Tab,
    pub scan: crate::scan::ScanUi,
    pub records: crate::records::RecordsUi,
    pub preview: crate::preview::PreviewUi,
    pub case: Option<Arc<CaseWorkspace>>,
    pub info: Option<CaseInfo>,
    pub scans: Vec<ScanInfo>,
    pub history_next: Option<storage::ScanCursor>,
    pub active_scan: Option<String>,
    pub notes: String,
    pub title: String,
    pub picker: Option<Picker>,
    pub settings: Settings,
    pub status: String,
    pub busy: bool,
    pub readonly: bool,
    sender: mpsc::SyncSender<Callback>,
    receiver: mpsc::Receiver<Callback>,
    smoke: bool,
    frames: u32,
}
impl Desktop {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        open: Option<PathBuf>,
        smoke: bool,
        korean: bool,
    ) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        cc.egui_ctx.set_style(style);
        let (sender, receiver) = mpsc::sync_channel(4);
        let mut app = Self {
            tab: Tab::Case,
            records: Default::default(),
            preview: Default::default(),
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
    fn chosen(&mut self, ctx: &egui::Context, purpose: Purpose, path: PathBuf) {
        match purpose {
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
    fn case_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Case workspace / 케이스 작업 공간");
        let Some(case) = self.case.clone() else {
            ui.add_space(28.0);
            ui.label(
                "Create or open a local case to begin. / 로컬 케이스를 만들거나 열어 시작하세요.",
            );
            ui.label("Metadata stays on your computer. / 메타데이터는 컴퓨터 안에서 처리됩니다.");
            ui.add_space(20.0);
            ui.heading("Recent cases / 최근 케이스");
            let mut selected = None;
            for path in &self.settings.recent {
                if let Ok(p) = path.to_path() {
                    if ui.button(p.display().to_string()).clicked() {
                        selected = Some(p);
                    }
                }
            }
            if let Some(path) = selected {
                self.open(ui.ctx(), path, false);
            }
            return;
        };
        if let Some(info) = &self.info {
            ui.label(format!("{}  ·  revision {}", info.case_id, info.revision));
        }
        ui.label(case.path().display().to_string());
        ui.separator();
        ui.label("Title / 제목");
        ui.text_edit_singleline(&mut self.title);
        ui.label("Case notes / 케이스 노트");
        ui.add(
            egui::TextEdit::multiline(&mut self.notes)
                .desired_rows(7)
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                !self.busy && !self.readonly,
                egui::Button::new("Save notes and title / 노트·제목 저장"),
            )
            .clicked()
        {
            let notes = self.notes.clone();
            let title = self.title.clone();
            self.task(
                ui.ctx(),
                "Saving case / 케이스 저장",
                move || {
                    case.set_title(&title)?;
                    case.set_notes(&notes)?;
                    case.info()
                },
                |app, info| app.info = Some(info),
            );
        }
        if self
            .info
            .as_ref()
            .is_some_and(|i| i.notes != self.notes || i.title != self.title)
        {
            ui.label("Unsaved case edits / 저장하지 않은 수정");
            if ui.button("Revert edits / 수정 되돌리기").clicked() {
                if let Some(info) = &self.info {
                    self.title = info.title.clone();
                    self.notes = info.notes.clone();
                }
            }
        }
        ui.add_space(16.0);
        ui.heading("Scan history / 스캔 기록");
        ui.horizontal(|ui| {
            for (label, next) in [("Latest / 최근", false), ("Older / 이전", true)] {
                if ui
                    .add_enabled(
                        !self.busy && (!next || self.history_next.is_some()),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    let case = self.case.as_ref().expect("open case").clone();
                    let cursor = if next {
                        self.history_next.clone()
                    } else {
                        None
                    };
                    self.task(
                        ui.ctx(),
                        "Loading history / 기록 읽기",
                        move || case.scan_history(cursor.as_ref(), 128),
                        |app, page| {
                            app.scans = page.items;
                            app.history_next = page.next;
                        },
                    );
                }
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("case-history")
            .show(ui, |ui| {
                for scan in &self.scans {
                    if ui
                        .selectable_label(
                            self.active_scan.as_ref() == Some(&scan.id),
                            format!(
                                "{} · {:?} · {} records · {}",
                                scan.id, scan.status, scan.record_count, scan.source
                            ),
                        )
                        .clicked()
                    {
                        self.active_scan = Some(scan.id.clone());
                    }
                }
            });
    }
}
impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && (self.busy || self.scan.running()) {
            self.scan.pause();
            self.scan.close_when_idle = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.status = "Saving checkpoint before closing / 체크포인트 저장 후 종료".into();
        }
        while let Ok(callback) = self.receiver.try_recv() {
            self.busy = false;
            callback(self);
        }
        self.poll_scan(ctx);
        self.poll_records(ctx);
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("MetaXtract").color(egui::Color32::from_rgb(99, 195, 255)),
                );
                ui.separator();
                for (label, purpose) in [
                    ("New case / 새 케이스", Purpose::New),
                    ("Open / 열기", Purpose::Open),
                    ("Save copy / 복사 저장", Purpose::Copy),
                ] {
                    if ui
                        .add_enabled(
                            !self.busy
                                && !self.scan.running()
                                && !self.info.as_ref().is_some_and(|i| {
                                    i.notes != self.notes || i.title != self.title
                                })
                                && (self.case.is_some() || !matches!(purpose, Purpose::Copy)),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.picker = Some(Picker::new(purpose));
                    }
                }
                if self.case.is_none() {
                    ui.checkbox(&mut self.readonly, "Read only / 읽기 전용");
                }
                if self.busy {
                    ui.spinner();
                }
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(&self.status);
                if self.readonly {
                    ui.strong("READ ONLY / 읽기 전용");
                }
            });
        });
        egui::SidePanel::left("navigation")
            .default_width(180.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.label("WORKSPACE / 작업 공간");
                ui.selectable_value(&mut self.tab, Tab::Case, "Case / 케이스");
                ui.selectable_value(&mut self.tab, Tab::Scan, "Scan queue / 스캔 대기열");
                ui.selectable_value(&mut self.tab, Tab::Records, "Records / 레코드");
                if let Some(scan) = &self.active_scan {
                    ui.separator();
                    ui.label("Selected scan / 선택 스캔");
                    ui.label(scan);
                }
            });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Case => self.case_view(ui),
            Tab::Scan => self.scan_view(ui),
            Tab::Records => {
                egui::ScrollArea::vertical()
                    .id_salt("record-content")
                    .show(ui, |ui| self.records_view(ui));
            }
        });
        if let Some(picker) = &mut self.picker {
            let purpose = picker.purpose;
            let (open, path) = picker.show(ctx);
            if !open || path.is_some() {
                self.picker = None;
            }
            if let Some(path) = path {
                self.chosen(ctx, purpose, path);
            }
        }
        if self.smoke {
            self.frames += 1;
            if self.frames >= 4 && !self.busy && !self.scan.running() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
    }
}
