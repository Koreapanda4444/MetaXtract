use crate::files::{Picker, Purpose};
pub use crate::state::{Desktop, Tab};
use eframe::egui;
impl Desktop {
    fn case_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, crate::i18n::tr("Case workspace"));
        let Some(case) = self.case.clone() else {
            ui.add_space(28.0);
            crate::components::heading(ui, crate::i18n::tr("Quick analysis"));
            ui.label(crate::i18n::tr(
                "Open a file or folder to inspect metadata immediately. No case setup is required.",
            ));
            if ui
                .button(crate::i18n::tr("Open for quick analysis"))
                .clicked()
            {
                self.picker = Some(Picker::new(Purpose::Quick));
            }
            self.recovery_view(ui);
            ui.add_space(16.0);
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
                self.chosen(ui.ctx(), Purpose::Open, path);
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
        if ctx.input(|i| i.viewport().close_requested())
            && self.temporary_session.is_some()
            && !self.session_ui.discard_once
            && !self.smoke
            && !self.busy
            && !self.scan.running()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.session_ui.confirm_close = true;
        }
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
        self.dropped_files(ctx);
        self.poll_scan(ctx);
        self.poll_records(ctx);
        self.poll_analysis(ctx);
        self.session_notice(ctx);
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
                    self.status = crate::i18n::tr(if self.busy || self.scan.running() {
                        "Working"
                    } else {
                        "Ready"
                    })
                    .into();
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
                    (crate::i18n::tr("Quick analysis"), Purpose::Quick),
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
        egui::TopBottomPanel::top("workspace-tools").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in [
                    (Tab::Records, "Files"),
                    (Tab::Case, "Case"),
                    (Tab::Scan, "Scan queue"),
                    (Tab::Search, "Search & compare"),
                    (Tab::Analysis, "Analysis"),
                    (Tab::Workflows, "Export & tools"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, crate::i18n::tr(label));
                }
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                crate::components::status(ui, &self.status, self.busy || self.scan.running());
                if ui.small_button(crate::i18n::tr("Notifications")).clicked() {
                    self.notifications.open = true;
                }
                if self.readonly {
                    ui.strong(crate::i18n::tr("READ ONLY"));
                }
            });
        });
        let navigation = egui::SidePanel::left("navigation")
            .default_width(self.settings.workspace.navigation_width.clamp(150.0, 360.0))
            .width_range(150.0..=360.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.explorer_view(ui);
            });
        self.settings.workspace.navigation_width = navigation.response.rect.width();
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
            Tab::Records => self.workspace_view(ui),
        });
        if let Some(picker) = &mut self.picker {
            let purpose = picker.purpose;
            let (open, paths) = picker.show(ctx);
            if !open || !paths.is_empty() {
                self.picker = None;
            }
            self.chosen_many(ctx, purpose, paths);
        }
        self.persist_pending_queue();
        self.save_display_preferences(ctx, ctx.input(|i| i.viewport().close_requested()));
        self.notifications.show(ctx);
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
