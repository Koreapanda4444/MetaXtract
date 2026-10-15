use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{self as app, core::scan::ScanControl, storage::ScanStatus};
use std::{
    collections::VecDeque,
    io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
struct Active {
    control: ScanControl,
    events: mpsc::Receiver<app::scan::AppScanEvent>,
    result: mpsc::Receiver<io::Result<app::scan::ScanOutcome>>,
    started: Instant,
}
pub struct ScanUi {
    pub source: String,
    pub source_path: Option<PathBuf>,
    pub workers: usize,
    pub queue: VecDeque<PathBuf>,
    active: Option<Active>,
    pub latest: Option<app::scan::AppScanEvent>,
    pub held: bool,
    pub close_when_idle: bool,
    pub outcome: Option<app::scan::ScanOutcome>,
    refresh_needed: bool,
}
impl Default for ScanUi {
    fn default() -> Self {
        Self {
            source: String::new(),
            source_path: None,
            workers: 2,
            queue: VecDeque::new(),
            active: None,
            latest: None,
            held: false,
            close_when_idle: false,
            outcome: None,
            refresh_needed: false,
        }
    }
}
impl ScanUi {
    pub fn running(&self) -> bool {
        self.active.is_some()
    }
    pub fn pause(&mut self) {
        self.held = true;
        if let Some(active) = &self.active {
            active.control.pause();
        }
    }
}
impl Desktop {
    pub fn start_scan(
        &mut self,
        ctx: &egui::Context,
        source: PathBuf,
        resume: Option<(u64, String, PathBuf)>,
    ) {
        if self.busy || self.scan.running() || self.readonly {
            return;
        }
        let Some(case) = self.case.as_ref() else {
            self.status = "Open a case first / 먼저 케이스를 여세요".into();
            return;
        };
        let mut request = app::scan::ScanRequest::new(source, case.path().to_owned());
        request.workers = self.scan.workers;
        if let Some((run, scan, journal)) = resume {
            request.resume = Some((run, scan));
            request.journal_path = journal;
        }
        let control = ScanControl::new();
        let worker_control = control.clone();
        let (sender, events) = mpsc::sync_channel(64);
        let (finished, result) = mpsc::sync_channel(1);
        let context = ctx.clone();
        match std::thread::Builder::new()
            .name("metaxtract-scan".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    app::scan::run(&request, worker_control, |event| {
                        let _ = sender.try_send(event);
                        context.request_repaint();
                        Ok(())
                    })
                }))
                .unwrap_or_else(|_| Err(io::Error::other("scan task panicked")));
                let _ = finished.send(result);
                context.request_repaint();
            }) {
            Ok(_) => {
                self.scan.active = Some(Active {
                    control,
                    events,
                    result,
                    started: Instant::now(),
                });
                self.scan.latest = None;
                self.scan.outcome = None;
                self.status = "Scanning / 스캔 중".into();
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub fn poll_scan(&mut self, ctx: &egui::Context) {
        let mut finished = None;
        if let Some(active) = &self.scan.active {
            while let Ok(event) = active.events.try_recv() {
                self.active_scan = Some(event.scan_id.clone());
                self.scan.latest = Some(event);
            }
            match active.result.try_recv() {
                Ok(out) => finished = Some(out),
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(Err(io::Error::other("scan task disconnected")))
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(result) = finished {
            self.scan.active = None;
            match result {
                Ok(out) => {
                    self.scan.held |= out.summary.stopped.is_some();
                    self.active_scan = Some(out.scan_id.clone());
                    self.status = format!(
                        "{} complete, {} failed · {:?} / 완료·실패·정지",
                        out.summary.completed, out.summary.failed, out.summary.stopped
                    );
                    self.scan.outcome = Some(out);
                    if let (Some(event), Some(outcome)) =
                        (&mut self.scan.latest, &self.scan.outcome)
                    {
                        event.summary = outcome.summary.clone();
                        event.kind = "finished".into();
                        event.path = None;
                    }
                }
                Err(e) => {
                    self.scan.held = true;
                    self.status = e.to_string();
                }
            }
            self.scan.refresh_needed = true;
        }
        if self.scan.refresh_needed && !self.busy {
            self.scan.refresh_needed = false;
            if let Some(case) = self.case.clone() {
                self.task(
                    ctx,
                    "Refreshing case / 케이스 갱신",
                    move || Ok((case.info()?, case.scan_history(None, 128)?)),
                    |app, (info, history)| {
                        app.info = Some(info);
                        app.scans = history.items;
                        app.history_next = history.next;
                    },
                );
            }
        }
        if !self.scan.running() && !self.busy && !self.scan.held && !self.scan.close_when_idle {
            if let Some(path) = self.scan.queue.pop_front() {
                self.start_scan(ctx, path, None);
            }
        }
        if self.scan.close_when_idle && !self.scan.running() && !self.busy {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    pub fn scan_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scan queue / 스캔 대기열");
        ui.label("Choose a file or directory. Results are saved as they arrive. / 파일·폴더를 선택하면 결과를 순서대로 저장합니다.");
        ui.horizontal(|ui| {
            if ui.text_edit_singleline(&mut self.scan.source).changed() {
                self.scan.source_path = None;
            }
            if ui.button("Browse / 찾기").clicked() {
                self.picker = Some(crate::files::Picker::new(crate::files::Purpose::Source));
            }
            ui.add(
                egui::DragValue::new(&mut self.scan.workers)
                    .range(1..=32)
                    .prefix("Workers / 작업자 "),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.case.is_some()
                        && !self.readonly
                        && !self.scan.source.is_empty()
                        && self.scan.queue.len() < 64,
                    egui::Button::new("Add to queue / 대기열 추가"),
                )
                .clicked()
            {
                let path = self
                    .scan
                    .source_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(&self.scan.source));
                self.scan.queue.push_back(path);
                self.scan.held = false;
            }
            if ui
                .add_enabled(self.scan.running(), egui::Button::new("Pause / 일시 정지"))
                .clicked()
            {
                self.scan.pause();
            }
            if ui
                .add_enabled(
                    self.scan.running(),
                    egui::Button::new("Cancel scan / 스캔 취소"),
                )
                .clicked()
            {
                self.scan.held = true;
                if let Some(active) = &self.scan.active {
                    active.control.cancel();
                }
            }
            if ui
                .button(if self.scan.held {
                    "Continue queue / 대기열 계속"
                } else {
                    "Hold queue / 대기열 정지"
                })
                .clicked()
            {
                self.scan.held = !self.scan.held;
            }
        });
        if let Some(event) = &self.scan.latest {
            let s = &event.summary;
            ui.add(
                egui::ProgressBar::new(if s.discovered == 0 {
                    0.0
                } else {
                    s.completed as f32 / s.discovered as f32
                })
                .show_percentage(),
            );
            ui.label(format!(
                "{} / {} files · {} failures · {} bytes hashed",
                s.completed, s.discovered, s.failed, s.bytes_hashed
            ));
            ui.label(event.path.as_deref().unwrap_or(&event.kind));
            if let Some(error) = &event.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        }
        if let Some(active) = &self.scan.active {
            ui.label(format!(
                "Elapsed / 경과: {:.1}s",
                active.started.elapsed().as_secs_f64()
            ));
        }
        egui::ScrollArea::vertical()
            .id_salt("scan-queue")
            .max_height(160.0)
            .show(ui, |ui| {
                let mut remove = None;
                for (i, path) in self.scan.queue.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("Remove / 제거").clicked() {
                            remove = Some(i);
                        }
                        ui.label(path.display().to_string());
                    });
                }
                if let Some(i) = remove {
                    self.scan.queue.remove(i);
                }
            });
        ui.separator();
        ui.heading("Resume checkpoint / 체크포인트 재개");
        ui.label("Paused or interrupted scans keep their original journal and source. / 정지·중단된 스캔은 원래 저널과 원본 경로가 필요합니다.");
        let resumable: Vec<_> = self
            .scans
            .iter()
            .filter(|s| matches!(s.status, ScanStatus::Paused | ScanStatus::Running))
            .map(|s| (s.id.clone(), s.source.clone()))
            .collect();
        for (id, source) in resumable {
            if ui
                .add_enabled(
                    !self.scan.running() && !self.busy && !self.readonly,
                    egui::Button::new(format!("Resume / 재개 · {id} · {source}")),
                )
                .clicked()
            {
                let result = (|| -> io::Result<_> {
                    let case = self
                        .case
                        .as_ref()
                        .ok_or_else(|| io::Error::other("no case"))?;
                    let receipt = case.scan_receipt(&id, "application.session")?.ok_or_else(
                        || {
                            io::Error::other(
                                "This scan has no checkpoint / 이 스캔에는 체크포인트가 없습니다",
                            )
                        },
                    )?;
                    let run = receipt["run"]
                        .as_u64()
                        .ok_or_else(|| io::Error::other("invalid checkpoint run"))?;
                    let journal: app::core::scan::NativePath =
                        serde_json::from_value(receipt["journal"].clone())
                            .map_err(io::Error::other)?;
                    let root: app::core::scan::NativePath =
                        serde_json::from_value(receipt["root"].clone())
                            .map_err(io::Error::other)?;
                    Ok((run, journal.to_path()?, root.to_path()?))
                })();
                match result {
                    Ok((run, journal, root)) => {
                        self.scan.held = false;
                        self.start_scan(ui.ctx(), root, Some((run, id, journal)));
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
        }
    }
}
