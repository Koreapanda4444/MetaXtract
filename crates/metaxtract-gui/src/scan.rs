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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueState {
    Completed,
    Failed,
    Paused,
    Cancelled,
}
impl QueueState {
    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Paused => "Paused",
            Self::Cancelled => "Cancelled",
        })
    }
}
struct Active {
    control: ScanControl,
    events: mpsc::Receiver<app::scan::AppScanEvent>,
    result: mpsc::Receiver<io::Result<app::scan::ScanOutcome>>,
    started: Instant,
}
pub struct ScanUi {
    pub source: String,
    pub active_source: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub workers: usize,
    pub decode_previews: bool,
    pub include_hidden: bool,
    pub cache_enabled: bool,
    pub max_files: u64,
    pub queue: VecDeque<PathBuf>,
    pub selected_queue: std::collections::BTreeSet<PathBuf>,
    pub queue_history: VecDeque<(PathBuf, QueueState)>,
    pub recent_files: VecDeque<app::scan::AppScanEvent>,
    pub skipped: u64,
    pub discovery_complete: bool,
    pub elapsed: Duration,
    pub history_filter: Option<QueueState>,
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
            active_source: None,
            source_path: None,
            workers: 2,
            decode_previews: false,
            include_hidden: false,
            cache_enabled: true,
            max_files: 1_000_000,
            queue: VecDeque::new(),
            selected_queue: Default::default(),
            queue_history: VecDeque::new(),
            recent_files: VecDeque::new(),
            skipped: 0,
            discovery_complete: false,
            elapsed: Duration::ZERO,
            history_filter: None,
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
    pub fn move_selected(&mut self, up: bool) {
        if up {
            for i in 1..self.queue.len() {
                if self.selected_queue.contains(&self.queue[i])
                    && !self.selected_queue.contains(&self.queue[i - 1])
                {
                    self.queue.swap(i, i - 1);
                }
            }
        } else {
            for i in (0..self.queue.len().saturating_sub(1)).rev() {
                if self.selected_queue.contains(&self.queue[i])
                    && !self.selected_queue.contains(&self.queue[i + 1])
                {
                    self.queue.swap(i, i + 1);
                }
            }
        }
    }
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
            self.status = crate::i18n::tr("Open a case first").into();
            return;
        };
        self.scan.active_source = Some(source.clone());
        let mut request = app::scan::ScanRequest::new(source, case.path().to_owned());
        request.workers = self.scan.workers;
        request.options.recursive.preview = self.scan.decode_previews;
        request.options.walk.include_hidden = self.scan.include_hidden;
        request.options.walk.max_files = self.scan.max_files;
        request.cache_enabled = self.scan.cache_enabled;
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
                        context.request_repaint();
                        sender
                            .send(event)
                            .map_err(|_| io::Error::other("scan window disconnected"))
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
                self.scan.recent_files.clear();
                self.scan.skipped = 0;
                self.scan.discovery_complete = false;
                self.scan.elapsed = Duration::ZERO;
                self.scan.outcome = None;
                self.status = crate::i18n::tr("Scanning").into();
            }
            Err(e) => self.report_error(e),
        }
    }
    pub fn poll_scan(&mut self, ctx: &egui::Context) {
        let mut finished = None;
        if let Some(active) = &self.scan.active {
            while let Ok(event) = active.events.try_recv() {
                self.active_scan = Some(event.scan_id.clone());
                self.scan.discovery_complete |=
                    event.kind == "discovery_finished" || event.kind == "finished";
                self.scan.skipped += u64::from(event.notice_code.as_deref().is_some_and(|code| {
                    matches!(code, "walk.symlink" | "walk.cycle" | "walk.special")
                }));
                if event.path.is_some() || event.message.is_some() {
                    self.scan.recent_files.retain(|old| old.path != event.path);
                    self.scan.recent_files.push_front(event.clone());
                    self.scan.recent_files.truncate(256);
                }
                self.scan.latest = Some(event);
            }
            match active.result.try_recv() {
                Ok(out) => finished = Some(out),
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(Err(io::Error::other("scan task disconnected")))
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            self.scan.elapsed = active.started.elapsed();
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(result) = finished {
            self.scan.discovery_complete = true;
            self.scan.active = None;
            let completed_source = self.scan.active_source.take();
            self.analysis.metrics_loaded = false;
            match result {
                Ok(out) => {
                    self.scan.held |= out.summary.stopped.is_some();
                    let state = match out.summary.stopped {
                        Some(app::core::scan::StopReason::Cancelled) => QueueState::Cancelled,
                        Some(_) => QueueState::Paused,
                        None if out.summary.failed > 0 => QueueState::Failed,
                        None => QueueState::Completed,
                    };
                    if let Some(path) = completed_source {
                        self.scan.queue_history.push_front((path, state));
                        self.scan.queue_history.truncate(128);
                    }
                    self.active_scan = Some(out.scan_id.clone());
                    self.status = crate::textf!(
                        "{} complete, {} failed · {:?}",
                        "완료 {}개, 실패 {}개 · {:?}",
                        out.summary.completed,
                        out.summary.failed,
                        out.summary.stopped
                    );
                    self.notifications.push(
                        self.status.clone(),
                        String::new(),
                        out.summary.failed > 0,
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
                    if let Some(path) = completed_source {
                        self.scan
                            .queue_history
                            .push_front((path, QueueState::Failed));
                        self.scan.queue_history.truncate(128);
                    }
                    self.report_error(e);
                }
            }
            if self.temporary_session.is_some() && self.scan.queue.is_empty() {
                self.tab = crate::state::Tab::Records;
            }
            self.scan.refresh_needed = true;
        }
        if self.scan.refresh_needed && !self.busy {
            self.scan.refresh_needed = false;
            if let Some(case) = self.case.clone() {
                self.task(
                    ctx,
                    crate::i18n::tr("Refreshing case"),
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
        crate::components::heading(ui, crate::i18n::tr("Scan queue"));
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.scan.include_hidden,
                crate::i18n::tr("Include hidden names"),
            );
            ui.checkbox(
                &mut self.scan.cache_enabled,
                crate::i18n::tr("Reuse verified metadata cache"),
            );
            ui.add(
                egui::DragValue::new(&mut self.scan.max_files)
                    .range(1..=1_000_000)
                    .prefix(crate::i18n::tr("File limit")),
            );
        });
        ui.checkbox(
            &mut self.scan.decode_previews,
            crate::i18n::tr("Decode previews for visual similarity"),
        );
        ui.label(crate::i18n::tr(
            "Choose a file or directory. Results are saved as they arrive.",
        ));
        ui.horizontal(|ui| {
            if crate::components::path_input(ui, &mut self.scan.source).changed() {
                self.scan.source_path = None;
            }
            if ui.button(crate::i18n::tr("Browse")).clicked() {
                self.picker = Some(crate::files::Picker::new(crate::files::Purpose::Source));
            }
            ui.add(
                egui::DragValue::new(&mut self.scan.workers)
                    .range(1..=32)
                    .prefix(crate::i18n::tr("Workers")),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.case.is_some()
                        && !self.readonly
                        && !self.scan.source.is_empty()
                        && self.scan.queue.len() < 64,
                    crate::components::button(crate::i18n::tr("Add to queue")),
                )
                .clicked()
            {
                let path = self
                    .scan
                    .source_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(&self.scan.source));
                self.ingest_paths(ui.ctx(), vec![path]);
            }
            if ui
                .add_enabled(
                    self.scan.running(),
                    crate::components::button(crate::i18n::tr("Pause")),
                )
                .clicked()
            {
                self.scan.pause();
            }
            if ui
                .add_enabled(
                    self.scan.running(),
                    crate::components::button(crate::i18n::tr("Cancel scan")),
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
                    crate::i18n::tr("Continue queue")
                } else {
                    crate::i18n::tr("Hold queue")
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
                    (s.completed as f32 / s.discovered as f32).clamp(0.0, 1.0)
                })
                .show_percentage()
                .animate(!self.scan.discovery_complete),
            );
            ui.label(crate::textf!(
                "{} / {} files · {} failures · {} bytes hashed",
                "{} / {}개 파일 · 실패 {} · 해시 처리 {}바이트",
                s.completed,
                s.discovered,
                s.failed,
                s.bytes_hashed
            ));
            ui.label(crate::textf!(
                "{} notices · {} skipped · {} unstable files",
                "안내 {} · 건너뜀 {} · 변경 감지 파일 {}",
                s.notices,
                self.scan.skipped,
                s.unstable
            ));
            ui.label(crate::i18n::tr(if self.scan.discovery_complete {
                "Discovery finished"
            } else {
                "Discovering files; progress may change"
            }));
            ui.label(event.path.as_deref().unwrap_or(&event.kind));
            if let Some(error) = &event.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        }
        ui.label(crate::textf!(
            "Elapsed: {:.1}s",
            "경과: {:.1}초",
            self.scan.elapsed.as_secs_f64()
        ));
        ui.collapsing(crate::i18n::tr("Per-file progress (latest 256)"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for event in &self.scan.recent_files {
                        let state = if event.error.is_some() {
                            "Failed"
                        } else {
                            match event.kind.as_str() {
                                "discovered" => "Pending",
                                "started" => "Running",
                                "completed" => "Completed",
                                "resumed" => "Resumed",
                                _ => "Notice",
                            }
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(crate::i18n::tr(state));
                            ui.label(event.path.as_deref().unwrap_or(""));
                            if let Some(detail) = event.error.as_ref().or(event.message.as_ref()) {
                                ui.add(egui::Label::new(detail).selectable(true).wrap());
                            }
                        });
                    }
                });
        });
        if let Some(outcome) = &self.scan.outcome {
            ui.collapsing(crate::i18n::tr("Scan failures"), |ui| {
                for error in &outcome.errors {
                    ui.label(format!("{}: {}", error.path, error.message));
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Select all queued")).clicked() {
                self.scan.selected_queue = self.scan.queue.iter().cloned().collect();
            }
            if ui.button(crate::i18n::tr("Remove selected")).clicked() {
                self.scan
                    .queue
                    .retain(|p| !self.scan.selected_queue.contains(p));
                self.scan.selected_queue.clear();
            }
            for (label, up) in [("Move up", true), ("Move down", false)] {
                if ui.button(crate::i18n::tr(label)).clicked() {
                    self.scan.move_selected(up);
                }
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("scan-queue")
            .max_height(180.0)
            .show(ui, |ui| {
                if let Some(path) = &self.scan.active_source {
                    ui.strong(format!(
                        "{}: {}",
                        crate::i18n::tr("Running"),
                        path.display()
                    ));
                }
                for (i, path) in self.scan.queue.iter().enumerate() {
                    let mut selected = self.scan.selected_queue.contains(path);
                    if ui
                        .checkbox(
                            &mut selected,
                            format!(
                                "{} · {} · {}",
                                i + 1,
                                crate::i18n::tr("Pending"),
                                path.display()
                            ),
                        )
                        .changed()
                    {
                        if selected {
                            self.scan.selected_queue.insert(path.clone());
                        } else {
                            self.scan.selected_queue.remove(path);
                        }
                    }
                }
            });
        ui.collapsing(crate::i18n::tr("Queue history"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.scan.history_filter, None, crate::i18n::tr("All"));
                for state in [
                    QueueState::Completed,
                    QueueState::Failed,
                    QueueState::Paused,
                    QueueState::Cancelled,
                ] {
                    ui.selectable_value(&mut self.scan.history_filter, Some(state), state.label());
                }
            });
            let mut retry = None;
            for (path, state) in &self.scan.queue_history {
                if self
                    .scan
                    .history_filter
                    .is_some_and(|filter| filter != *state)
                {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.label(format!("{} · {}", state.label(), path.display()));
                    if matches!(state, QueueState::Failed | QueueState::Cancelled)
                        && ui.small_button(crate::i18n::tr("Analyze again")).clicked()
                    {
                        retry = Some(path.clone());
                    }
                });
            }
            if let Some(path) = retry {
                self.ingest_paths(ui.ctx(), vec![path]);
            }
        });
        ui.separator();
        crate::components::heading(ui, crate::i18n::tr("Resume checkpoint"));
        ui.label(crate::i18n::tr(
            "Paused or interrupted scans keep their original journal and source.",
        ));
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
                    crate::components::button(crate::textf!(
                        "Resume · {id} · {source}",
                        "재개 · {id} · {source}"
                    )),
                )
                .clicked()
            {
                let result = (|| -> io::Result<_> {
                    let case = self
                        .case
                        .as_ref()
                        .ok_or_else(|| io::Error::other("no case"))?;
                    let receipt =
                        case.scan_receipt(&id, "application.session")?
                            .ok_or_else(|| {
                                io::Error::other(crate::i18n::tr("This scan has no checkpoint"))
                            })?;
                    let run = receipt["run"]
                        .as_u64()
                        .ok_or_else(|| io::Error::other("invalid checkpoint run"))?;
                    let journal: app::core::scan::NativePath =
                        serde_json::from_value(receipt["journal"].clone())
                            .map_err(io::Error::other)?;
                    let root: app::core::scan::NativePath =
                        serde_json::from_value(receipt["root"].clone())
                            .map_err(io::Error::other)?;
                    let mut sibling = case.path().as_os_str().to_os_string();
                    sibling.push(".scan.sqlite");
                    let sibling = PathBuf::from(sibling);
                    let journal = if sibling.is_file() {
                        sibling
                    } else {
                        journal.to_path()?
                    };
                    Ok((run, journal, root.to_path()?))
                })();
                match result {
                    Ok((run, journal, root)) => {
                        self.scan.held = false;
                        self.start_scan(ui.ctx(), root, Some((run, id, journal)));
                    }
                    Err(e) => self.report_error(e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_queue_reordering_preserves_selected_order_and_boundaries() {
        let mut queue = ScanUi {
            queue: ["a", "b", "c", "d"].map(PathBuf::from).into(),
            ..Default::default()
        };
        queue
            .selected_queue
            .extend([PathBuf::from("b"), PathBuf::from("c")]);
        queue.move_selected(true);
        assert_eq!(queue.queue, ["b", "c", "a", "d"].map(PathBuf::from));
        queue.move_selected(true);
        assert_eq!(queue.queue, ["b", "c", "a", "d"].map(PathBuf::from));
        queue.move_selected(false);
        assert_eq!(queue.queue, ["a", "b", "c", "d"].map(PathBuf::from));
        queue.move_selected(false);
        assert_eq!(queue.queue, ["a", "d", "b", "c"].map(PathBuf::from));
    }
}
