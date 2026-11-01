use crate::desktop::{Desktop, Tab};
use eframe::egui;
use metaxtract_app::{
    self as app,
    schema::ExtractionRecord,
    storage::{PageCursor, RecordHeader, RecordOrder, SearchQuery},
};
use std::{
    collections::BTreeSet,
    io,
    path::PathBuf,
    time::{Duration, Instant},
};
pub struct RecordsUi {
    pending_edge: Option<bool>,
    pub action: Option<crate::file_actions::FileAction>,
    pub query: Option<SearchQuery>,
    pub headers: Vec<RecordHeader>,
    pub next: Option<PageCursor>,
    cursor: Option<PageCursor>,
    back: Vec<Option<PageCursor>>,
    pub total: u64,
    pub revision: u64,
    pub order: RecordOrder,
    pub group_by: usize,
    pub mime_filter: String,
    pub status_filter: Option<app::schema::ExtractionStatus>,
    pub selected: BTreeSet<i64>,
    pub focused: Option<(i64, ExtractionRecord)>,
    pub note: String,
    pub tags: String,
    pub refresh: bool,
    loaded: Instant,
}
impl Default for RecordsUi {
    fn default() -> Self {
        Self {
            pending_edge: None,
            action: None,
            query: None,
            headers: vec![],
            next: None,
            cursor: None,
            back: vec![],
            total: 0,
            revision: 0,
            order: RecordOrder::Path,
            group_by: 0,
            mime_filter: String::new(),
            status_filter: None,
            selected: BTreeSet::new(),
            focused: None,
            note: String::new(),
            tags: String::new(),
            refresh: true,
            loaded: Instant::now(),
        }
    }
}
impl Desktop {
    pub fn poll_records(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Records || self.busy {
            return;
        }
        let Some(scan) = self.active_scan.clone() else {
            return;
        };
        if self
            .records
            .query
            .as_ref()
            .is_none_or(|q| q.records.scan_id != scan)
        {
            let order = self.records.order;
            let group_by = self.records.group_by;
            self.records = RecordsUi::default();
            self.records.order = order;
            self.records.group_by = group_by;
            let mut q = app::analysis::search::parse(&scan, "").expect("empty query");
            q.records.order = self.records.order;
            self.records.query = Some(q);
        }
        if self.records.refresh
            || (self.scan.running()
                && self.records.cursor.is_none()
                && self.records.loaded.elapsed() > Duration::from_millis(700))
        {
            self.load_records(ctx, None, 0);
        }
        if !self.busy && self.temporary_session.is_some() && self.records.focused.is_none() {
            if let Some(record) = self.records.headers.first() {
                self.focus_record(ctx, record.id);
            }
        }
    }
    fn load_records(&mut self, ctx: &egui::Context, cursor: Option<PageCursor>, direction: i8) {
        if self.busy {
            return;
        }
        let (Some(case), Some(mut query)) = (self.case.clone(), self.records.query.clone()) else {
            return;
        };
        query.records.order = self.records.order;
        self.records.refresh = false;
        let context = ctx.clone();
        self.task(
            ctx,
            crate::i18n::tr("Loading results"),
            move || {
                let (page, restarted) = match case.search_records(&query, cursor.as_ref(), 64) {
                    Ok(page) => (page, false),
                    Err(e) if cursor.is_some() && e.kind() == io::ErrorKind::InvalidData => {
                        (case.search_records(&query, None, 64)?, true)
                    }
                    Err(e) => return Err(e),
                };
                let total = case.count_search(&query)?;
                let revision = case.info()?.revision;
                Ok((page, total, revision, query, cursor, restarted))
            },
            move |app, (page, total, revision, query, cursor, restarted)| {
                if restarted || direction == 0 {
                    app.records.back.clear();
                    app.records.cursor = None;
                } else if direction > 0 {
                    app.records.back.push(app.records.cursor.take());
                    app.records.cursor = cursor;
                } else {
                    app.records.back.pop();
                    app.records.cursor = cursor;
                }
                app.records.query = Some(query);
                app.records.headers = page.items;
                app.records.next = page.next;
                app.records.total = total;
                app.records.revision = revision;
                app.records.loaded = Instant::now();
                if let Some(last) = app.records.pending_edge.take() {
                    if let Some(id) = if last {
                        app.records.headers.last()
                    } else {
                        app.records.headers.first()
                    }
                    .map(|h| h.id)
                    {
                        app.focus_record(&context, id);
                    }
                }
                if restarted {
                    app.status =
                        crate::i18n::tr("Case changed; results restarted at the first page.")
                            .into();
                }
            },
        );
    }
    pub(crate) fn focus_record(&mut self, ctx: &egui::Context, id: i64) {
        if let Some(case) = self.case.clone() {
            let context = ctx.clone();
            self.task(
                ctx,
                crate::i18n::tr("Loading record"),
                move || {
                    let r = case
                        .record(id)?
                        .ok_or_else(|| io::Error::other("record disappeared"))?;
                    let annotation = case.annotation(id)?;
                    let mut analyzed = r.clone();
                    let analysis_error = match app::scan::analyze_record(&mut analyzed) {
                        Ok(()) => None,
                        Err(e) => Some(e.to_string()),
                    };
                    if analysis_error.is_none() {
                        Ok((analyzed, annotation, analysis_error))
                    } else {
                        Ok((r, annotation, analysis_error))
                    }
                },
                move |app, (record, annotation, analysis_error)| {
                    if let Some(error) = analysis_error {
                        app.status = format!(
                            "Analysis incomplete; original fields remain available: {error}"
                        );
                    }
                    app.records.focused = Some((id, record));
                    app.records.note = annotation
                        .as_ref()
                        .map(|a| a.note.clone())
                        .unwrap_or_default();
                    app.records.tags = annotation.map(|a| a.tags.join(", ")).unwrap_or_default();
                    app.load_linked_preview(&context);
                },
            );
        }
    }
    pub fn export_selection(&mut self, ctx: &egui::Context, path: PathBuf, csv: bool) {
        let Some(case) = self.case.clone() else {
            return;
        };
        let Some(scan) = self.active_scan.clone() else {
            return;
        };
        match app::workflows::selected_query(&scan, &self.records.selected) {
            Ok(q) => self.task(
                ctx,
                crate::i18n::tr("Exporting selection"),
                move || {
                    app::analysis::export::export_records(
                        &case,
                        &q,
                        path,
                        if csv {
                            app::analysis::export::ExportFormat::Csv
                        } else {
                            app::analysis::export::ExportFormat::Jsonl
                        },
                        None,
                        Default::default(),
                    )
                },
                |app, r| {
                    app.status =
                        crate::textf!("Exported {} records", "{}개 레코드 내보냄", r.records)
                },
            ),
            Err(e) => self.report_error(e),
        }
    }
    pub fn records_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, crate::i18n::tr("Evidence records"));
        if self.active_scan.is_none() {
            ui.label(crate::i18n::tr("Choose a scan in Case history."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let before = self.records.order;
            egui::ComboBox::from_id_salt("record-order")
                .selected_text(crate::i18n::tr(match self.records.order {
                    RecordOrder::Path => "Name ascending",
                    RecordOrder::PathDescending => "Name descending",
                    RecordOrder::Size => "Size ascending",
                    RecordOrder::SizeDescending => "Size descending",
                    RecordOrder::MimePath => "Type then name",
                    RecordOrder::MimeSize => "Type then size",
                    RecordOrder::StatusPath => "Status then name",
                    RecordOrder::StatusSize => "Status then size",
                    _ => "Discovery order",
                }))
                .show_ui(ui, |ui| {
                    for (label, order) in [
                        ("Name ascending", RecordOrder::Path),
                        ("Name descending", RecordOrder::PathDescending),
                        ("Size ascending", RecordOrder::Size),
                        ("Size descending", RecordOrder::SizeDescending),
                        ("Type then name", RecordOrder::MimePath),
                        ("Type then size", RecordOrder::MimeSize),
                        ("Status then name", RecordOrder::StatusPath),
                        ("Status then size", RecordOrder::StatusSize),
                        ("Discovery order", RecordOrder::Insertion),
                    ] {
                        ui.selectable_value(&mut self.records.order, order, crate::i18n::tr(label));
                    }
                });
            if before != self.records.order {
                self.records.refresh = true;
            }
            if ui.button(crate::i18n::tr("Refresh")).clicked() {
                self.records.refresh = true;
            }
            ui.label(crate::textf!(
                "{} records · page {} · {} selected",
                "{}개 레코드 · {}페이지 · {}개 선택",
                self.records.total,
                self.records.back.len() + 1,
                self.records.selected.len()
            ));
        });
        self.file_filters_view(ui);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.records.back.is_empty(),
                    crate::components::button(crate::i18n::tr("Previous")),
                )
                .clicked()
            {
                self.load_records(ui.ctx(), self.records.back.last().cloned().flatten(), -1);
            }
            if ui
                .add_enabled(
                    !self.busy && self.records.next.is_some(),
                    crate::components::button(crate::i18n::tr("Next")),
                )
                .clicked()
            {
                self.load_records(ui.ctx(), self.records.next.clone(), 1);
            }
            if ui.button(crate::i18n::tr("Select page")).clicked() {
                for h in &self.records.headers {
                    if self.records.selected.len() == 64 {
                        break;
                    }
                    self.records.selected.insert(h.id);
                }
            }
            if ui.button(crate::i18n::tr("Clear selection")).clicked() {
                self.records.selected.clear();
            }
        });
        let mut focus = None;
        egui::ScrollArea::horizontal()
            .id_salt("records-columns")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (width, label) in [
                        (24.0, "✓"),
                        (210.0, "Filename"),
                        (115.0, "Type"),
                        (90.0, "Size"),
                        (176.0, "Modified (UTC)"),
                        (100.0, "Status"),
                    ] {
                        ui.add_sized(
                            [width, 24.0],
                            egui::Label::new(egui::RichText::new(crate::i18n::tr(label)).strong()),
                        );
                    }
                });
                ui.separator();
                if self.records.group_by == 0 {
                    egui::ScrollArea::vertical()
                        .id_salt("records-table")
                        .max_height((ui.clip_rect().height() - 190.0).max(120.0))
                        .show_rows(ui, 30.0, self.records.headers.len(), |ui, range| {
                            for index in range {
                                if let Some(id) = self.file_row(ui, index) {
                                    focus = Some(id);
                                }
                            }
                        });
                } else {
                    let mut groups = std::collections::BTreeMap::<String, Vec<usize>>::new();
                    for (i, h) in self.records.headers.iter().enumerate() {
                        groups
                            .entry(if self.records.group_by == 1 {
                                h.mime.clone()
                            } else {
                                format!("{:?}", h.status)
                            })
                            .or_default()
                            .push(i);
                    }
                    ui.small(crate::i18n::tr(
                        "Groups contain the current page; sorting applies to the entire result.",
                    ));
                    egui::ScrollArea::vertical()
                        .id_salt("grouped-files")
                        .max_height((ui.clip_rect().height() - 190.0).max(120.0))
                        .show(ui, |ui| {
                            for (name, indices) in groups {
                                egui::CollapsingHeader::new(format!(
                                    "{} · {}",
                                    crate::i18n::tr(&name),
                                    indices.len()
                                ))
                                .id_salt(("file-group", &name))
                                .default_open(true)
                                .show(ui, |ui| {
                                    for index in indices {
                                        if let Some(id) = self.file_row(ui, index) {
                                            focus = Some(id);
                                        }
                                    }
                                });
                            }
                        });
                }
            });
        if let Some(action) = self.records.action.take() {
            self.file_action(ui.ctx(), action);
        }
        if let Some(id) = focus {
            self.focus_record(ui.ctx(), id);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (label, purpose) in [
                (crate::i18n::tr("Selected CSV"), crate::files::Purpose::Csv),
                (
                    crate::i18n::tr("Selected JSONL"),
                    crate::files::Purpose::Jsonl,
                ),
            ] {
                if ui
                    .add_enabled(
                        !self.busy && !self.records.selected.is_empty(),
                        crate::components::button(label),
                    )
                    .clicked()
                {
                    self.picker = Some(crate::files::Picker::new(purpose));
                }
            }
            ui.label(crate::i18n::tr(
                "Selection limit: 64 · checkboxes or Ctrl/Cmd+click",
            ));
        });
        ui.collapsing(crate::i18n::tr("Annotations"), |ui| {
            ui.label(crate::i18n::tr("Note and comma-separated tags"));
            ui.text_edit_singleline(&mut self.records.note);
            ui.text_edit_singleline(&mut self.records.tags);
            if ui
                .add_enabled(
                    !self.busy && !self.readonly && !self.records.selected.is_empty(),
                    crate::components::button(crate::i18n::tr("Replace selected annotations")),
                )
                .clicked()
            {
                if let Some(case) = self.case.clone() {
                    let ids = self.records.selected.iter().copied().collect::<Vec<_>>();
                    let note = self.records.note.clone();
                    let tags = self
                        .records
                        .tags
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    self.task(
                        ui.ctx(),
                        crate::i18n::tr("Saving annotations"),
                        move || case.annotate_many(&ids, &note, &tags),
                        |app, n| {
                            app.records.refresh = true;
                            app.status =
                                crate::textf!("Updated {n} annotations", "주석 {n}개 저장");
                        },
                    );
                }
            }
        });
    }
    fn file_row(&mut self, ui: &mut egui::Ui, index: usize) -> Option<i64> {
        let mut focus = None;
        let h = &self.records.headers[index];
        ui.push_id(h.id, |ui| {
                        let focused = self.records.focused.as_ref().is_some_and(|r| r.0 == h.id);
                        let fill = if focused { ui.visuals().selection.bg_fill } else if index % 2 == 0 { ui.visuals().faint_bg_color } else { egui::Color32::TRANSPARENT };
                        egui::Frame::new().fill(fill).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut selected = self.records.selected.contains(&h.id);
                                if ui.add_enabled_ui(selected || self.records.selected.len() < 64, |ui| ui.add_sized([24.0, 24.0], egui::Checkbox::without_text(&mut selected))).inner.changed() {
                                    if selected { self.records.selected.insert(h.id); } else { self.records.selected.remove(&h.id); }
                                }
                                let name = h.path.rsplit('/').next().unwrap_or(&h.path);
                                let row = ui.add_sized([210.0,24.0], egui::Button::selectable(focused,name).truncate());
                                if row.clicked() { focus = Some(h.id); }
                                row.clone().context_menu(|ui| {
                                    ui.add_enabled_ui(!self.busy, |ui| {
                                        for (label,action) in [("Show file details",crate::file_actions::FileAction::Inspect(h.id)),("Copy path",crate::file_actions::FileAction::CopyPath(h.path.clone())),("Export CSV",crate::file_actions::FileAction::Export(h.id,true)),("Export JSONL",crate::file_actions::FileAction::Export(h.id,false))] {
                                            if ui.button(crate::i18n::tr(label)).clicked() {self.records.action=Some(action); ui.close();}
                                        }
                                        if !self.readonly && ui.button(crate::i18n::tr("Analyze selected again")).clicked() {
                                            let ids=if self.records.selected.contains(&h.id) {self.records.selected.iter().copied().collect()} else {vec![h.id]};
                                            self.records.action=Some(crate::file_actions::FileAction::Reanalyze(ids)); ui.close();
                                        }
                                    });
                                });
                                row.on_hover_text(&h.path);
                                ui.add_sized([115.0,24.0],egui::Label::new(&h.mime).truncate()).on_hover_text(&h.mime);
                                ui.add_sized([90.0,24.0],egui::Label::new(crate::formatting::bytes(h.size_bytes))).on_hover_text(h.size_bytes.to_string());
                                ui.add_sized([176.0,24.0],egui::Label::new(crate::formatting::modified(h.modified_ns.as_deref()))).on_hover_text(crate::i18n::tr("Filesystem time; unknown dates remain blank. Full original values are in file details."));
                                let color = match h.status { app::schema::ExtractionStatus::Complete => egui::Color32::LIGHT_GREEN, app::schema::ExtractionStatus::Failed => egui::Color32::LIGHT_RED, app::schema::ExtractionStatus::Partial => egui::Color32::YELLOW, _ => ui.visuals().weak_text_color() };
                                ui.add_sized([100.0,24.0],egui::Label::new(egui::RichText::new(crate::i18n::tr(&format!("{:?}", h.status))).color(color)));
                            });
                        });
                    });
        focus
    }
    fn file_filters_view(&mut self, ui: &mut egui::Ui) {
        let mut apply = false;
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.records.mime_filter)
                        .desired_width(150.0)
                        .hint_text(crate::i18n::tr("MIME type (all if empty)")),
                );
                egui::ComboBox::from_id_salt("file-status-filter")
                    .selected_text(
                        self.records
                            .status_filter
                            .map(|s| crate::i18n::tr(&format!("{s:?}")).to_owned())
                            .unwrap_or_else(|| crate::i18n::tr("All statuses").into()),
                    )
                    .show_ui(ui, |ui| {
                        apply |= ui
                            .selectable_value(
                                &mut self.records.status_filter,
                                None,
                                crate::i18n::tr("All statuses"),
                            )
                            .changed();
                        for status in [
                            app::schema::ExtractionStatus::Complete,
                            app::schema::ExtractionStatus::Partial,
                            app::schema::ExtractionStatus::Failed,
                            app::schema::ExtractionStatus::Unsupported,
                        ] {
                            apply |= ui
                                .selectable_value(
                                    &mut self.records.status_filter,
                                    Some(status),
                                    crate::i18n::tr(&format!("{status:?}")),
                                )
                                .changed();
                        }
                    });
                apply |= ui.button(crate::i18n::tr("Apply filters")).clicked();
                egui::ComboBox::from_id_salt("file-grouping")
                    .selected_text(crate::i18n::tr(
                        ["No grouping", "Group by type", "Group by status"][self.records.group_by],
                    ))
                    .show_ui(ui, |ui| {
                        for (i, label) in ["No grouping", "Group by type", "Group by status"]
                            .iter()
                            .enumerate()
                        {
                            if ui
                                .selectable_value(
                                    &mut self.records.group_by,
                                    i,
                                    crate::i18n::tr(label),
                                )
                                .changed()
                            {
                                self.records.order = match i {
                                    1 => RecordOrder::MimePath,
                                    2 => RecordOrder::StatusPath,
                                    _ => RecordOrder::Path,
                                };
                                apply = true;
                            }
                        }
                    });
            });
        });
        if apply {
            if let Some(q) = &mut self.records.query {
                q.records.mime = if self.records.mime_filter.trim().is_empty() {
                    None
                } else {
                    Some(self.records.mime_filter.trim().to_ascii_lowercase())
                };
                q.records.status = self.records.status_filter;
                self.records.refresh = true;
                self.records.selected.clear();
                self.records.focused = None;
            }
        }
    }
    fn step_file(&mut self, ctx: &egui::Context, forward: bool) {
        let position = self
            .records
            .focused
            .as_ref()
            .and_then(|r| self.records.headers.iter().position(|h| h.id == r.0));
        let target = match (position, forward) {
            (Some(i), true) => i.checked_add(1).filter(|i| *i < self.records.headers.len()),
            (Some(i), false) => i.checked_sub(1),
            (None, true) => Some(0),
            (None, false) => self.records.headers.len().checked_sub(1),
        };
        if let Some(id) = target
            .and_then(|i| self.records.headers.get(i))
            .map(|h| h.id)
        {
            self.focus_record(ctx, id);
        } else if forward && self.records.next.is_some() {
            self.records.pending_edge = Some(false);
            self.load_records(ctx, self.records.next.clone(), 1);
        } else if !forward && !self.records.back.is_empty() {
            self.records.pending_edge = Some(true);
            self.load_records(ctx, self.records.back.last().cloned().flatten(), -1);
        }
    }
    pub fn record_detail_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, forward) in [("Previous file", false), ("Next file", true)] {
                if ui
                    .add_enabled(
                        !self.busy && !self.records.headers.is_empty(),
                        crate::components::button(crate::i18n::tr(label)),
                    )
                    .clicked()
                {
                    self.step_file(ui.ctx(), forward);
                }
            }
        });
        if self.records.focused.is_none() {
            ui.label(crate::i18n::tr(
                "Select a file to inspect metadata and preview it.",
            ));
        }
        if let Some((id, record)) = &self.records.focused {
            ui.separator();
            ui.label(crate::textf!(
                "Record {id} · {} raw fields · {} claims · {} diagnostics",
                "레코드 {id} · 원본 필드 {} · 해석 {} · 진단 {}",
                record.observations.len(),
                record.claims.len(),
                record.diagnostics.len()
            ));
            self.inspector_view(ui);
        }
    }
}
