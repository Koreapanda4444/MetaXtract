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
    pub query: Option<SearchQuery>,
    pub headers: Vec<RecordHeader>,
    pub next: Option<PageCursor>,
    cursor: Option<PageCursor>,
    back: Vec<Option<PageCursor>>,
    pub total: u64,
    pub revision: u64,
    pub order: RecordOrder,
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
            query: None,
            headers: vec![],
            next: None,
            cursor: None,
            back: vec![],
            total: 0,
            revision: 0,
            order: RecordOrder::Path,
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
            self.records = RecordsUi::default();
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
        self.task(ctx,"Loading results / 결과 읽기",move||{
            let (page,restarted)=match case.search_records(&query,cursor.as_ref(),64){
                Ok(page)=>(page,false),Err(e)if cursor.is_some()&&e.kind()==io::ErrorKind::InvalidData=>(case.search_records(&query,None,64)?,true),Err(e)=>return Err(e),
            };
            let total=case.count_search(&query)?;let revision=case.info()?.revision;
            Ok((page,total,revision,query,cursor,restarted))
        },move|app,(page,total,revision,query,cursor,restarted)|{
            if restarted||direction==0{app.records.back.clear();app.records.cursor=None;}
            else if direction>0{app.records.back.push(app.records.cursor.take());app.records.cursor=cursor;}
            else{app.records.back.pop();app.records.cursor=cursor;}
            app.records.query=Some(query);app.records.headers=page.items;app.records.next=page.next;app.records.total=total;app.records.revision=revision;app.records.loaded=Instant::now();
            if restarted{app.status="Case changed; results restarted at the first page. / 케이스가 변경되어 첫 페이지로 돌아왔습니다.".into();}
        });
    }
    fn focus_record(&mut self, ctx: &egui::Context, id: i64) {
        if let Some(case) = self.case.clone() {
            self.task(
                ctx,
                "Loading record / 레코드 읽기",
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
                "Exporting selection / 선택 내보내기",
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
                    app.status = format!(
                        "Exported {} records / {}개 레코드 내보냄",
                        r.records, r.records
                    )
                },
            ),
            Err(e) => self.status = e.to_string(),
        }
    }
    pub fn records_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Evidence records / 증거 레코드");
        if self.active_scan.is_none() {
            ui.label("Choose a scan in Case history. / 케이스 기록에서 스캔을 선택하세요.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let before = self.records.order;
            for (label, order) in [
                ("Path ↑ / 경로", RecordOrder::Path),
                ("Size ↑ / 크기", RecordOrder::Size),
                ("Discovery ↑ / 발견 순", RecordOrder::Insertion),
            ] {
                ui.selectable_value(&mut self.records.order, order, label);
            }
            if before != self.records.order {
                self.records.refresh = true;
            }
            if ui.button("Refresh / 새로 고침").clicked() {
                self.records.refresh = true;
            }
            ui.label(format!(
                "{} records · page {} · {} selected",
                self.records.total,
                self.records.back.len() + 1,
                self.records.selected.len()
            ));
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.records.back.is_empty(),
                    egui::Button::new("Previous / 이전"),
                )
                .clicked()
            {
                self.load_records(ui.ctx(), self.records.back.last().cloned().flatten(), -1);
            }
            if ui
                .add_enabled(
                    !self.busy && self.records.next.is_some(),
                    egui::Button::new("Next / 다음"),
                )
                .clicked()
            {
                self.load_records(ui.ctx(), self.records.next.clone(), 1);
            }
            if ui.button("Select page / 페이지 선택").clicked() {
                for h in &self.records.headers {
                    if self.records.selected.len() == 64 {
                        break;
                    }
                    self.records.selected.insert(h.id);
                }
            }
            if ui.button("Clear selection / 선택 해제").clicked() {
                self.records.selected.clear();
            }
        });
        let mut focus = None;
        egui::ScrollArea::both()
            .id_salt("records-table")
            .max_height(290.0)
            .show(ui, |ui| {
                egui::Grid::new("record-rows")
                    .striped(true)
                    .min_row_height(27.0)
                    .show(ui, |ui| {
                        ui.label("✓");
                        ui.strong("Path / 경로");
                        ui.strong("MIME");
                        ui.strong("Bytes / 크기");
                        ui.strong("Status / 상태");
                        ui.end_row();
                        for h in &self.records.headers {
                            let mut selected = self.records.selected.contains(&h.id);
                            if ui
                                .add_enabled(
                                    selected || self.records.selected.len() < 64,
                                    egui::Checkbox::without_text(&mut selected),
                                )
                                .changed()
                            {
                                if selected {
                                    self.records.selected.insert(h.id);
                                } else {
                                    self.records.selected.remove(&h.id);
                                }
                            }
                            let row = ui.add_sized(
                                [340.0, 24.0],
                                egui::Button::selectable(
                                    self.records.focused.as_ref().is_some_and(|r| r.0 == h.id),
                                    egui::RichText::new(&h.path),
                                )
                                .truncate(),
                            );
                            if row.clicked() {
                                focus = Some(h.id);
                                if ui.input(|i| i.modifiers.command)
                                    && !self.records.selected.remove(&h.id)
                                    && self.records.selected.len() < 64
                                {
                                    self.records.selected.insert(h.id);
                                }
                            }
                            row.on_hover_text(&h.path);
                            ui.label(&h.mime);
                            ui.label(h.size_bytes.to_string());
                            ui.label(format!("{:?}", h.status));
                            ui.end_row();
                        }
                    });
            });
        if let Some(id) = focus {
            self.focus_record(ui.ctx(), id);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (label, purpose) in [
                ("Selected CSV / 선택 CSV", crate::files::Purpose::Csv),
                ("Selected JSONL / 선택 JSONL", crate::files::Purpose::Jsonl),
            ] {
                if ui
                    .add_enabled(
                        !self.busy && !self.records.selected.is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.picker = Some(crate::files::Picker::new(purpose));
                }
            }
            ui.label("Selection limit: 64 · checkboxes or Ctrl/Cmd+click / 최대 64개 선택");
        });
        ui.collapsing("Annotations / 주석", |ui| {
            ui.label("Note and comma-separated tags / 노트와 쉼표로 구분한 태그");
            ui.text_edit_singleline(&mut self.records.note);
            ui.text_edit_singleline(&mut self.records.tags);
            if ui
                .add_enabled(
                    !self.busy && !self.readonly && !self.records.selected.is_empty(),
                    egui::Button::new("Replace selected annotations / 선택 주석 교체"),
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
                        "Saving annotations / 주석 저장",
                        move || case.annotate_many(&ids, &note, &tags),
                        |app, n| {
                            app.records.refresh = true;
                            app.status = format!("Updated {n} annotations / 주석 {n}개 저장");
                        },
                    );
                }
            }
        });
        if let Some((id, record)) = &self.records.focused {
            ui.separator();
            ui.label(format!(
                "Record {id} · {} raw fields · {} claims · {} diagnostics",
                record.observations.len(),
                record.claims.len(),
                record.diagnostics.len()
            ));
            self.inspector_view(ui);
        }
    }
}
