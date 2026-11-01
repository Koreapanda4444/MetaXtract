use crate::files::{Picker, Purpose};
pub use crate::state::{Desktop, Tab};
use eframe::egui;
use metaxtract_app::{self as app, storage};
impl Desktop {
    fn case_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, crate::i18n::tr("Case workspace"));
        let Some(case) = self.case.clone() else {
            ui.add_space(28.0);
            ui.label(crate::i18n::tr("Create or open a local case to begin."));
            ui.label(crate::i18n::tr("Metadata stays on your computer."));
            ui.add_space(20.0);
            crate::components::heading(ui, crate::i18n::tr("Recent cases"));
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
        ui.label(crate::i18n::tr("Title"));
        ui.text_edit_singleline(&mut self.title);
        ui.label(crate::i18n::tr("Case notes"));
        ui.add(
            egui::TextEdit::multiline(&mut self.notes)
                .desired_rows(7)
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                !self.busy && !self.readonly,
                crate::components::button(crate::i18n::tr("Save notes and title")),
            )
            .clicked()
        {
            let notes = self.notes.clone();
            let title = self.title.clone();
            self.task(
                ui.ctx(),
                crate::i18n::tr("Saving case"),
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
            ui.label(crate::i18n::tr("Unsaved case edits"));
            if ui.button(crate::i18n::tr("Revert edits")).clicked() {
                if let Some(info) = &self.info {
                    self.title = info.title.clone();
                    self.notes = info.notes.clone();
                }
            }
        }
        ui.add_space(16.0);
        crate::components::heading(ui, crate::i18n::tr("Scan history"));
        ui.horizontal(|ui| {
            for (label, next) in [
                (crate::i18n::tr("Latest"), false),
                (crate::i18n::tr("Older"), true),
            ] {
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
                        crate::i18n::tr("Loading history"),
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
            self.status = crate::i18n::tr("Saving checkpoint before closing").into();
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
                let mut language = crate::i18n::language();
                egui::ComboBox::from_id_salt("language")
                    .selected_text(language.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut language, crate::i18n::Language::Korean, "한국어");
                        ui.selectable_value(
                            &mut language,
                            crate::i18n::Language::English,
                            "English",
                        );
                    });
                if language != crate::i18n::language() {
                    crate::i18n::set(language);
                    self.settings.language = Some(language);
                    if let Err(error) = self.settings.save() {
                        self.status = error.to_string();
                    }
                }
                ui.menu_button(crate::i18n::tr("Appearance"), |ui| {
                    let before = (self.settings.appearance, self.settings.scale);
                    for (theme, label) in [
                        (crate::settings::Appearance::System, "System"),
                        (crate::settings::Appearance::Dark, "Dark"),
                        (crate::settings::Appearance::Light, "Light"),
                    ] {
                        ui.selectable_value(
                            &mut self.settings.appearance,
                            theme,
                            crate::i18n::tr(label),
                        );
                    }
                    ui.add(
                        egui::Slider::new(&mut self.settings.scale, 0.75..=2.0)
                            .text(crate::i18n::tr("UI scale")),
                    );
                    if before != (self.settings.appearance, self.settings.scale) {
                        self.settings.appearance.apply(ctx, self.settings.scale);
                        if let Err(error) = self.settings.save() {
                            self.status = error.to_string();
                        }
                    }
                });
                for (label, purpose) in [
                    (crate::i18n::tr("New case"), Purpose::New),
                    (crate::i18n::tr("Open"), Purpose::Open),
                    (crate::i18n::tr("Save copy"), Purpose::Copy),
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
                    ui.checkbox(&mut self.readonly, crate::i18n::tr("Read only"));
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
                    ui.strong(crate::i18n::tr("READ ONLY"));
                }
            });
        });
        egui::SidePanel::left("navigation")
            .default_width(180.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.label(crate::i18n::tr("WORKSPACE"));
                ui.selectable_value(&mut self.tab, Tab::Case, crate::i18n::tr("Case"));
                ui.selectable_value(&mut self.tab, Tab::Scan, crate::i18n::tr("Scan queue"));
                ui.selectable_value(&mut self.tab, Tab::Records, crate::i18n::tr("Records"));
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Search,
                    crate::i18n::tr("Search & compare"),
                );
                ui.selectable_value(&mut self.tab, Tab::Analysis, crate::i18n::tr("Analysis"));
                ui.selectable_value(
                    &mut self.tab,
                    Tab::Workflows,
                    crate::i18n::tr("Export & tools"),
                );
                if let Some(scan) = &self.active_scan {
                    ui.separator();
                    ui.label(crate::i18n::tr("Selected scan"));
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
