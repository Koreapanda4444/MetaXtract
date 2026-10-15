use crate::{
    desktop::{Desktop, Tab},
    inspectors::json_view,
};
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::diff::ScanDiff,
    storage::{self as s, FilterQuery, SearchQuery},
};
use std::io;
#[derive(Default)]
pub struct SearchUi {
    pub text: String,
    pub advanced: bool,
    pub json: String,
    pub name: String,
    pub filters: Vec<s::SavedFilter>,
    pub facets: Vec<(String, s::FacetPage)>,
    pub before: String,
    pub after: String,
    pub comparison: Option<ScanDiff>,
    pub comparison_revision: u64,
    comparison_scope: (String, String, String),
    pub compare: bool,
    view: usize,
    selected_record: Option<usize>,
    selected_field: Option<usize>,
}
impl Desktop {
    fn search_query(&self) -> io::Result<SearchQuery> {
        let scan = self
            .active_scan
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "select a scan"))?;
        let mut q = if self.search.advanced {
            if self.search.json.len() > 65536 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "typed query exceeds 64 KiB",
                ));
            }
            serde_json::from_str::<SearchQuery>(&self.search.json).map_err(io::Error::other)?
        } else {
            app::analysis::search::parse(scan, &self.search.text)?
        };
        q.records.scan_id = scan.clone();
        q.records.order = self.records.order;
        q.validate()?;
        Ok(q)
    }
    fn apply_search(&mut self, mut q: SearchQuery) {
        q.records.order = self.records.order;
        self.active_scan = Some(q.records.scan_id.clone());
        self.records = Default::default();
        self.records.order = q.records.order;
        self.records.query = Some(q);
        self.records.refresh = true;
        self.tab = Tab::Records;
    }
    fn load_filter(&mut self, filter: FilterQuery) -> io::Result<()> {
        let scan = self
            .active_scan
            .clone()
            .ok_or_else(|| io::Error::other("select a scan"))?;
        let mut q = match filter {
            FilterQuery::Structured(q) => q,
            FilterQuery::Records(records) => {
                let mut q = SearchQuery::new(&scan, s::SearchExpression::All(vec![]));
                q.records = records;
                q
            }
            FilterQuery::Legacy { query, status } => {
                let mut q = app::analysis::search::parse(&scan, &query)?;
                match status.to_ascii_lowercase().as_str() {
                    "" | "all" => {}
                    "ok" | "success" => {
                        q.records.status = Some(app::schema::ExtractionStatus::Complete)
                    }
                    "error" | "failed" => {
                        q.records.status = Some(app::schema::ExtractionStatus::Failed)
                    }
                    "warning" | "partial" => {
                        q.records.status = Some(app::schema::ExtractionStatus::Partial)
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "legacy status requires manual review; original filter is retained",
                        ));
                    }
                };
                q
            }
        };
        q.records.scan_id = scan;
        q.validate()?;
        self.search.json = serde_json::to_string_pretty(&q).map_err(io::Error::other)?;
        self.search.advanced = true;
        self.apply_search(q);
        Ok(())
    }
    fn refresh_filters(&mut self, ctx: &egui::Context, older: bool) {
        if let Some(case) = self.case.clone() {
            let after = if older {
                self.search.filters.last().map(|f| f.name.clone())
            } else {
                None
            };
            self.task(
                ctx,
                "Loading filters / 필터 읽기",
                move || case.filters(after.as_deref(), 128),
                |app, filters| app.search.filters = filters,
            );
        }
    }
    pub fn search_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.search.compare, false, "Search / 검색");
            ui.selectable_value(&mut self.search.compare, true, "Compare scans / 스캔 비교");
        });
        ui.separator();
        if self.search.compare {
            self.comparison_view(ui);
            return;
        }
        ui.heading("Structured metadata search / 구조화 메타데이터 검색");
        if self.active_scan.is_none() {
            ui.label("Select a scan first. / 스캔을 먼저 선택하세요.");
            return;
        }
        ui.label(
            "Examples / 예: mime:png size>1000 -status=Failed claim.image.width>=1920 tag:review",
        );
        let toggle = ui.checkbox(
            &mut self.search.advanced,
            "Typed query JSON (AND/OR/NOT) / 구조화 쿼리 JSON",
        );
        if toggle.changed() && self.search.advanced {
            match app::analysis::search::parse(
                self.active_scan.as_deref().unwrap_or(""),
                &self.search.text,
            )
            .and_then(|q| serde_json::to_string_pretty(&q).map_err(io::Error::other))
            {
                Ok(json) => self.search.json = json,
                Err(e) => self.status = e.to_string(),
            }
        }
        if self.search.advanced {
            ui.add(
                egui::TextEdit::multiline(&mut self.search.json)
                    .desired_rows(7)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        } else {
            ui.add(egui::TextEdit::singleline(&mut self.search.text).desired_width(f32::INFINITY));
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Apply / 적용"))
                .clicked()
            {
                match self.search_query() {
                    Ok(q) => self.apply_search(q),
                    Err(e) => self.status = e.to_string(),
                }
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Summarize matches / 검색 집계"),
                )
                .clicked()
            {
                match (self.case.clone(), self.search_query()) {
                    (Some(case), Ok(q)) => self.task(
                        ui.ctx(),
                        "Counting search facets / 검색 집계",
                        move || {
                            let revision = case.info()?.revision;
                            let mut facets = vec![];
                            for (name, field) in [
                                ("mime", s::FacetField::Mime),
                                ("status", s::FacetField::Status),
                                ("claim", s::FacetField::ClaimKey),
                                ("tag", s::FacetField::Tag),
                            ] {
                                facets.push((name.into(), case.search_facets(&q, field, 32)?));
                            }
                            if case.info()?.revision != revision {
                                return Err(io::Error::other("case changed; retry facet summary"));
                            }
                            Ok(facets)
                        },
                        |app, facets| app.search.facets = facets,
                    ),
                    (_, Err(e)) => self.status = e.to_string(),
                    _ => {}
                }
            }
        });
        for (name, page) in &self.search.facets {
            ui.collapsing(
                format!(
                    "{name} · {} groups{}",
                    page.values.len(),
                    if page.more {
                        " (more; narrow query)"
                    } else {
                        ""
                    }
                ),
                |ui| {
                    for v in &page.values {
                        ui.label(format!("{} · {}", v.records, v.value));
                    }
                },
            );
        }
        ui.separator();
        ui.heading("Saved filters / 저장 필터");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.search.name);
            if ui
                .add_enabled(
                    !self.busy && !self.readonly && !self.search.name.is_empty(),
                    egui::Button::new("Save current / 현재 필터 저장"),
                )
                .clicked()
            {
                match (self.case.clone(), self.search_query()) {
                    (Some(case), Ok(q)) => {
                        let name = self.search.name.clone();
                        self.task(
                            ui.ctx(),
                            "Saving filter / 필터 저장",
                            move || {
                                case.save_filter(&name, &FilterQuery::Structured(q))?;
                                Ok((case.info()?, case.filters(None, 128)?))
                            },
                            |app, (info, filters)| {
                                app.info = Some(info);
                                app.search.filters = filters;
                            },
                        );
                    }
                    (_, Err(e)) => self.status = e.to_string(),
                    _ => {}
                }
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Reload / 다시 읽기"))
                .clicked()
            {
                self.refresh_filters(ui.ctx(), false);
            }
            if ui
                .add_enabled(
                    !self.busy && self.search.filters.len() == 128,
                    egui::Button::new("More / 더 보기"),
                )
                .clicked()
            {
                self.refresh_filters(ui.ctx(), true);
            }
        });
        let mut load = None;
        let mut remove = None;
        for (i, filter) in self.search.filters.iter().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new(&filter.name))
                        .clicked()
                    {
                        load = Some(filter.query.clone());
                    }
                    if ui
                        .add_enabled(
                            !self.busy && !self.readonly,
                            egui::Button::new("Delete / 삭제"),
                        )
                        .clicked()
                    {
                        remove = Some(filter.name.clone());
                    }
                });
            });
        }
        if let Some(query) = load {
            if let Err(e) = self.load_filter(query) {
                self.status = e.to_string();
            }
        }
        for filter in &self.search.filters {
            if matches!(filter.query, FilterQuery::Legacy { .. }) {
                ui.collapsing(
                    format!("Legacy criteria / 이전 조건 · {}", filter.name),
                    |ui| json_view(ui, &filter.query),
                );
            }
        }
        if let (Some(name), Some(case)) = (remove, self.case.clone()) {
            self.task(
                ui.ctx(),
                "Removing filter / 필터 삭제",
                move || {
                    case.remove_filter(&name)?;
                    Ok((case.info()?, case.filters(None, 128)?))
                },
                |app, (info, filters)| {
                    app.info = Some(info);
                    app.search.filters = filters;
                },
            );
        }
    }
    fn comparison_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scan comparison / 스캔 비교");
        if self.search.before.is_empty() {
            self.search.before = self
                .scans
                .get(1)
                .or(self.scans.first())
                .map(|s| s.id.clone())
                .unwrap_or_default();
        }
        if self.search.after.is_empty() {
            self.search.after = self.active_scan.clone().unwrap_or_default();
        }
        ui.label("Scan IDs (available in Case history) / 케이스 기록의 스캔 ID");
        ui.horizontal(|ui| {
            ui.label("Before / 이전");
            ui.text_edit_singleline(&mut self.search.before);
            ui.label("After / 이후");
            ui.text_edit_singleline(&mut self.search.after);
        });
        ui.label("Optional shared search restriction / 두 스캔에 공통으로 적용할 검색 조건");
        ui.text_edit_singleline(&mut self.search.text);
        if ui
            .add_enabled(
                !self.busy && !self.scan.running(),
                egui::Button::new("Compare / 비교"),
            )
            .clicked()
        {
            if let Some(case) = self.case.clone() {
                let before = self.search.before.clone();
                let after = self.search.after.clone();
                let text = self.search.text.clone();
                let scope = (before.clone(), after.clone(), text.clone());
                self.task(
                    ui.ctx(),
                    "Comparing scans / 스캔 비교",
                    move || {
                        let revision = case.info()?.revision;
                        if case.scan_info(&before)?.is_none() || case.scan_info(&after)?.is_none() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "unknown comparison scan",
                            ));
                        }
                        let before =
                            app::collect(&case, &app::analysis::search::parse(&before, &text)?)?;
                        let after =
                            app::collect(&case, &app::analysis::search::parse(&after, &text)?)?;
                        let diff = app::analysis::diff::compare_scans(
                            &before,
                            &after,
                            Default::default(),
                        )?;
                        if case.info()?.revision != revision {
                            return Err(io::Error::other("case changed during comparison; retry"));
                        }
                        Ok((diff, revision))
                    },
                    move |app, (diff, revision)| {
                        app.search.comparison = Some(diff);
                        app.search.comparison_revision = revision;
                        app.search.comparison_scope = scope;
                        app.search.selected_record = None;
                        app.search.selected_field = None;
                    },
                );
            }
        }
        if self.scan.running() {
            ui.label("Pause the active scan for a stable comparison. / 일관된 비교를 위해 진행 중인 스캔을 정지하세요.");
        }
        if let Some(diff) = &self.search.comparison {
            ui.label(format!(
                "Compared / 비교한 스캔: {} → {} · query {:?}",
                self.search.comparison_scope.0,
                self.search.comparison_scope.1,
                self.search.comparison_scope.2
            ));
            ui.label(format!(
                "Revision {} · {} added · {} removed · {} changed · {} moved",
                self.search.comparison_revision,
                diff.added.len(),
                diff.removed.len(),
                diff.changed.len(),
                diff.relocations.len()
            ));
            ui.horizontal(|ui| {
                for (i, label) in [
                    "Changed / 변경",
                    "Added / 추가",
                    "Removed / 삭제",
                    "Moved / 이동",
                ]
                .iter()
                .enumerate()
                {
                    if ui
                        .selectable_value(&mut self.search.view, i, *label)
                        .changed()
                    {
                        self.search.selected_record = None;
                        self.search.selected_field = None;
                    }
                }
            });
            match self.search.view {
                0 => {
                    egui::ScrollArea::vertical()
                        .id_salt("diff-records")
                        .max_height(190.0)
                        .show_rows(ui, 26.0, diff.changed.len(), |ui, range| {
                            for i in range {
                                let r = &diff.changed[i];
                                if ui
                                    .selectable_label(
                                        self.search.selected_record == Some(i),
                                        format!(
                                            "{} · {} fields · content {:?}",
                                            r.after_path,
                                            r.fields.len(),
                                            r.content_changed
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.search.selected_record = Some(i);
                                    self.search.selected_field = None;
                                }
                            }
                        });
                    if let Some(record) = self
                        .search
                        .selected_record
                        .and_then(|i| diff.changed.get(i))
                    {
                        egui::ScrollArea::vertical()
                            .id_salt("diff-fields")
                            .max_height(160.0)
                            .show_rows(ui, 26.0, record.fields.len(), |ui, range| {
                                for i in range {
                                    if ui
                                        .selectable_label(
                                            self.search.selected_field == Some(i),
                                            format!("{:?}", record.fields[i].field),
                                        )
                                        .clicked()
                                    {
                                        self.search.selected_field = Some(i);
                                    }
                                }
                            });
                        if let Some(field) = self
                            .search
                            .selected_field
                            .and_then(|i| record.fields.get(i))
                        {
                            ui.columns(2, |columns| {
                                columns[0].strong("Before / 이전");
                                json_view(&mut columns[0], &field.before);
                                columns[1].strong("After / 이후");
                                json_view(&mut columns[1], &field.after);
                            });
                        }
                        ui.collapsing(
                            "Semantic claim changes / 의미 단위 해석 변경",
                            |ui| json_view(ui, &record.claims),
                        );
                    }
                }
                1 | 2 => {
                    let paths = if self.search.view == 1 {
                        &diff.added
                    } else {
                        &diff.removed
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("diff-paths")
                        .max_height(300.0)
                        .show_rows(ui, 26.0, paths.len(), |ui, range| {
                            for path in &paths[range] {
                                ui.label(path.to_string());
                            }
                        });
                }
                _ => {
                    egui::ScrollArea::vertical()
                        .id_salt("diff-relocations")
                        .max_height(300.0)
                        .show_rows(ui, 26.0, diff.relocations.len(), |ui, range| {
                            for moved in &diff.relocations[range] {
                                ui.label(format!("{} → {}", moved.before, moved.after));
                            }
                        });
                }
            }
        }
    }
}
