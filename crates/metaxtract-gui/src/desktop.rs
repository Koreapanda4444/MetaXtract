use crate::files::{Picker, Purpose};
pub use crate::state::{Desktop, Tab};
use eframe::egui;
use metaxtract_app::{self as app, storage};
impl Desktop {
    fn case_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, "Case workspace / 케이스 작업 공간");
        let Some(case) = self.case.clone() else {
            ui.add_space(28.0);
            ui.label(
                "Create or open a local case to begin. / 로컬 케이스를 만들거나 열어 시작하세요.",
            );
            ui.label("Metadata stays on your computer. / 메타데이터는 컴퓨터 안에서 처리됩니다.");
            ui.add_space(20.0);
            crate::components::heading(ui, "Recent cases / 최근 케이스");
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
                crate::components::button("Save notes and title / 노트·제목 저장"),
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
        crate::components::heading(ui, "Scan history / 스캔 기록");
        ui.horizontal(|ui| {
            for (label, next) in [("Latest / 최근", false), ("Older / 이전", true)] {
                if ui
                    .add_enabled(
                        !self.busy && (!next || self.history_next.is_some()),
                        crate::components::button(label),
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
        self.poll_analysis(ctx);
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                crate::components::heading(
                    ui,
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
                            crate::components::button(label),
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
                crate::components::status(ui, &self.status, self.busy || self.scan.running());
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
                ui.selectable_value(&mut self.tab, Tab::Search, "Search & compare / 검색·비교");
                ui.selectable_value(&mut self.tab, Tab::Analysis, "Analysis / 분석");
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Workflows,
                    "Export & tools / 내보내기·도구",
                );
                if let Some(scan) = &self.active_scan {
                    ui.separator();
                    ui.label("Selected scan / 선택 스캔");
                    ui.label(scan);
                }
            });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Case => self.case_view(ui),
            Tab::Scan => self.scan_view(ui),
            Tab::Workflows => {
                egui::ScrollArea::vertical()
                    .id_salt("workflows-content")
                    .show(ui, |ui| self.workflows_view(ui));
            }
            Tab::Analysis => {
                egui::ScrollArea::vertical()
                    .id_salt("analysis-content")
                    .show(ui, |ui| self.analysis_view(ui));
            }
            Tab::Search => {
                egui::ScrollArea::vertical()
                    .id_salt("search-content")
                    .show(ui, |ui| self.search_view(ui));
            }
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
