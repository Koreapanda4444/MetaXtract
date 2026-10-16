use crate::{
    desktop::{Desktop, Tab},
    files::{Picker, Purpose},
    inspectors::json_view,
};
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{self as a, reporting::InvestigationReport},
    storage::{CaseTotals, SearchQuery},
};
use std::{io, path::PathBuf};

pub struct AnalysisUi {
    totals: Option<CaseTotals>,
    revision: u64,
    scan_id: Option<String>,
    statistics: Option<serde_json::Value>,
    pub metrics_loaded: bool,
    report: Option<Box<InvestigationReport>>,
    selected: bool,
    view: usize,
    detail: usize,
    severity: usize,
    radius: f64,
    groups: String,
    zoom: f32,
    center: egui::Vec2,
}
impl Default for AnalysisUi {
    fn default() -> Self {
        Self {
            totals: None,
            revision: 0,
            scan_id: None,
            statistics: None,
            metrics_loaded: false,
            report: None,
            selected: false,
            view: 0,
            detail: 0,
            severity: 0,
            radius: 100.0,
            groups: "{}".into(),
            zoom: 1.0,
            center: egui::Vec2::ZERO,
        }
    }
}
impl Desktop {
    pub fn active_query(&self, selected: bool) -> io::Result<SearchQuery> {
        let scan = self
            .active_scan
            .as_deref()
            .ok_or_else(|| io::Error::other("Select a scan / 스캔을 선택하세요"))?;
        if selected {
            app::workflows::selected_query(scan, &self.records.selected)
        } else if let Some(q) = self
            .records
            .query
            .as_ref()
            .filter(|q| q.records.scan_id == scan)
        {
            Ok(q.clone())
        } else {
            a::search::parse(scan, "")
        }
    }
    pub fn poll_analysis(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Analysis || self.busy || self.scan.running() {
            return;
        }
        if self.analysis.metrics_loaded && self.analysis.scan_id == self.active_scan {
            return;
        }
        let Some(case) = self.case.clone() else {
            return;
        };
        let scan = self.active_scan.clone();
        self.analysis.metrics_loaded = true;
        self.analysis.scan_id = scan.clone();
        self.task(
            ctx,
            "Loading case totals / 케이스 통계 읽기",
            move || {
                let before = case.info()?.revision;
                let totals = case.totals()?;
                let statistics = scan
                    .as_deref()
                    .map(|s| case.scan_receipt(s, "application.summary"))
                    .transpose()?
                    .flatten();
                if before != case.info()?.revision {
                    return Err(io::Error::other(
                        "Case changed; refresh statistics / 통계를 새로 고치세요",
                    ));
                }
                Ok((totals, before, statistics))
            },
            |s, (totals, revision, statistics)| {
                s.analysis.totals = Some(totals);
                s.analysis.revision = revision;
                s.analysis.statistics = statistics;
            },
        );
    }
    pub fn analysis_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Case analysis / 케이스 분석");
        if self.case.is_none() {
            ui.label("Open a case first / 먼저 케이스를 여세요");
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh totals / 통계 갱신"))
                .clicked()
            {
                self.analysis.metrics_loaded = false;
            }
            ui.checkbox(
                &mut self.analysis.selected,
                "Selected records only / 선택 레코드만",
            );
            if ui
                .add_enabled(
                    !self.busy && !self.scan.running(),
                    egui::Button::new("Build analysis / 분석 실행"),
                )
                .clicked()
            {
                let result = (|| -> io::Result<_> {
                    let query = self.active_query(self.analysis.selected)?;
                    if self.analysis.groups.len() > 65536 {
                        return Err(io::Error::other("Track groups exceed 64 KiB"));
                    }
                    let mut options =
                        app::workflows::report_options("Case analysis / 케이스 분석", "")?;
                    options.cluster_radius_meters = self.analysis.radius;
                    options.track_groups =
                        serde_json::from_str(&self.analysis.groups).map_err(io::Error::other)?;
                    Ok((query, options))
                })();
                match result {
                    Ok((query, options)) => {
                        let case = self.case.clone().expect("open case");
                        self.task(
                            ui.ctx(),
                            "Analyzing metadata / 메타데이터 분석 중",
                            move || {
                                a::reporting::build_report(
                                    &case,
                                    &query,
                                    &options,
                                    None,
                                    None,
                                    Default::default(),
                                )
                                .map(Box::new)
                            },
                            |s, r| {
                                s.analysis.report = Some(r);
                                s.analysis.detail = 0;
                            },
                        );
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
        });
        ui.label("Scope uses the current record filter or selected IDs. Analysis is a revision-bound snapshot. / 현재 필터 또는 선택 ID를 분석하며 결과는 해당 시점의 스냅샷입니다.");
        ui.collapsing("Geographic options / 위치 분석 설정",|ui| {
            ui.horizontal(|ui| { ui.label("Cluster radius (m) / 군집 반경"); ui.add(egui::DragValue::new(&mut self.analysis.radius).range(0.0..=1_000_000.0)); });
            ui.label("Track groups: explicit {evidence path: group} JSON. Unassigned points stay unresolved. / 이동 경로는 {근거 경로: 그룹} JSON으로 지정하세요. 미지정 위치는 연결하지 않습니다.");
            ui.add(egui::TextEdit::multiline(&mut self.analysis.groups).desired_rows(2).desired_width(f32::INFINITY));
        });
        ui.horizontal_wrapped(|ui| {
            for (i, label) in [
                "Dashboard / 요약",
                "Timeline / 타임라인",
                "Locations / 위치",
                "Duplicates / 중복",
                "Findings / 발견 사항",
            ]
            .into_iter()
            .enumerate()
            {
                if ui
                    .selectable_value(&mut self.analysis.view, i, label)
                    .clicked()
                {
                    self.analysis.detail = 0;
                }
            }
        });
        ui.separator();
        if self.analysis.view == 0 {
            if let Some(t) = &self.analysis.totals {
                ui.label(format!(
                    "Whole case · revision {} / 전체 케이스",
                    self.analysis.revision
                ));
                egui::Grid::new("case-totals").striped(true).show(ui, |ui| {
                    for (label, v) in [
                        ("Scans / 스캔", t.scans),
                        ("Records / 레코드", t.records),
                        ("Raw fields / 원본 필드", t.observations),
                        ("Claims / 정규화 값", t.claims),
                        ("Conflicts / 충돌", t.conflicts),
                        ("Diagnostics / 진단", t.diagnostics),
                    ] {
                        ui.label(label);
                        ui.strong(v.to_string());
                        ui.end_row();
                    }
                });
                ui.collapsing("Persisted file statistics / 저장된 파일 통계",|ui| { if let Some(s)=&self.analysis.statistics { json_view(ui,s); } else { ui.label("Unavailable for older/imported scans / 이전·가져온 스캔에는 통계가 없습니다"); } });
            }
        }
        let Some(report) = self.analysis.report.as_ref() else {
            ui.label("Run analysis to populate these views / 분석을 실행하면 결과가 표시됩니다");
            return;
        };
        ui.label(format!(
            "Snapshot: {} · revision {} · {} / {} records",
            report.scan_id,
            report.case_revision,
            report.coverage.selected_records,
            report.coverage.full_scan_records
        ));
        if self.active_scan.as_ref() != Some(&report.scan_id)
            || self
                .info
                .as_ref()
                .is_some_and(|i| i.revision != report.case_revision)
        {
            ui.colored_label(egui::Color32::YELLOW,"Older snapshot; run analysis again for the current selection / 이전 분석 결과입니다. 현재 선택으로 다시 분석하세요.");
        }
        match self.analysis.view {
            0 => {
                json_view(ui, &report.coverage);
                for text in &report.limitations {
                    ui.label(text);
                }
            }
            1 => {
                ui.label("Unknown time zones remain unresolved; claims retain their original provenance. / 시간대가 없는 시각은 미확정으로 보존합니다.");
                egui::ScrollArea::vertical()
                    .id_salt("timeline-list")
                    .max_height(270.0)
                    .show_rows(ui, 24.0, report.timeline.len(), |ui, range| {
                        for i in range {
                            let e = &report.timeline[i];
                            if ui
                                .selectable_label(
                                    self.analysis.detail == i,
                                    format!("{} · {} · {:?}", e.path, e.event, e.timestamp),
                                )
                                .clicked()
                            {
                                self.analysis.detail = i;
                            }
                        }
                    });
                if let Some(e) = report.timeline.get(self.analysis.detail) {
                    json_view(ui, e);
                }
            }
            2 => self.location_view(ui),
            3 => {
                ui.label("Visual similarity is a heuristic. Enable Decode previews for a new scan to supply image fingerprints. / 시각적 유사성은 추정입니다. 새 스캔에서 미리보기 디코딩을 켜야 비교할 수 있습니다.");
                ui.collapsing(
                    format!(
                        "Exact duplicate groups / 해시 중복 그룹 ({})",
                        report.exact_duplicates.len()
                    ),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("exact-groups")
                            .max_height(240.0)
                            .show_rows(ui, 24.0, report.exact_duplicates.len(), |ui, range| {
                                for i in range {
                                    let g = &report.exact_duplicates[i];
                                    if ui
                                        .selectable_label(
                                            self.analysis.detail == i,
                                            format!(
                                                "{:?} · {} · {} files",
                                                g.algorithm,
                                                g.digest,
                                                g.paths.len()
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.analysis.detail = i;
                                    }
                                }
                            });
                        if let Some(g) = report.exact_duplicates.get(self.analysis.detail) {
                            json_view(ui, g);
                        }
                    },
                );
                ui.collapsing(
                    format!(
                        "Visual matches / 시각적 유사 ({})",
                        report.visually_similar.len()
                    ),
                    |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show_rows(
                            ui,
                            70.0,
                            report.visually_similar.len(),
                            |ui, range| {
                                for i in range {
                                    json_view(ui, &report.visually_similar[i]);
                                }
                            },
                        );
                    },
                );
                ui.label(format!(
                    "Images with fingerprints: {}; missing previews: {} / 지문 확보·미리보기 누락",
                    report.coverage.visual_records_with_previews,
                    report.coverage.visual_unavailable.len()
                ));
                ui.collapsing("Missing coverage / 분석 누락", |ui| {
                    egui::ScrollArea::vertical().max_height(180.0).show_rows(
                        ui,
                        22.0,
                        report.coverage.visual_unavailable.len(),
                        |ui, r| {
                            for i in r {
                                ui.label(report.coverage.visual_unavailable[i].to_string());
                            }
                        },
                    );
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    for (i, label) in [
                        "All / 전체",
                        "High / 높음",
                        "Medium / 중간",
                        "Low / 낮음",
                        "Info / 정보",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.analysis.severity, i, label);
                    }
                });
                let findings: Vec<_> = report
                    .records
                    .iter()
                    .flat_map(|r| &r.findings)
                    .filter(|f| {
                        self.analysis.severity == 0
                            || match f.severity {
                                a::findings::Severity::High => 1,
                                a::findings::Severity::Medium => 2,
                                a::findings::Severity::Low => 3,
                                a::findings::Severity::Information => 4,
                            } == self.analysis.severity
                    })
                    .collect();
                ui.label(format!("{} findings / 발견 사항", findings.len()));
                egui::ScrollArea::vertical()
                    .id_salt("finding-list")
                    .max_height(270.0)
                    .show_rows(ui, 24.0, findings.len(), |ui, range| {
                        for i in range {
                            let f = findings[i];
                            if ui
                                .selectable_label(
                                    self.analysis.detail == i,
                                    format!("{:?} · {} · {}", f.severity, f.rule, f.path),
                                )
                                .clicked()
                            {
                                self.analysis.detail = i;
                            }
                        }
                    });
                if let Some(f) = findings.get(self.analysis.detail) {
                    json_view(ui, f);
                }
            }
        }
    }
    fn location_view(&mut self, ui: &mut egui::Ui) {
        let report = self.analysis.report.as_ref().expect("analysis snapshot");
        ui.label("Offline coordinate plot · longitude / latitude · no network tiles / 오프라인 경도·위도 도표");
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.analysis.zoom, 1.0..=64.0)
                    .logarithmic(true)
                    .text("Zoom / 확대"),
            );
            if ui.button("Reset / 초기화").clicked() {
                self.analysis.zoom = 1.0;
                self.analysis.center = egui::Vec2::ZERO;
            }
            for (label, p) in [("GeoJSON", Purpose::GeoJson), ("KML", Purpose::Kml)] {
                if ui
                    .add_enabled(!self.busy, egui::Button::new(label))
                    .clicked()
                {
                    self.picker = Some(Picker::new(p));
                }
            }
        });
        let (response, painter) = ui.allocate_painter(
            egui::vec2(ui.available_width().max(100.0), 250.0),
            egui::Sense::click_and_drag(),
        );
        let rect = response.rect;
        if response.dragged() {
            self.analysis.center += response.drag_delta();
        }
        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(15, 25, 38));
        let project = |lon: f64, lat: f64| {
            rect.center()
                + self.analysis.center
                + egui::vec2(
                    lon as f32 / 360.0 * rect.width(),
                    -lat as f32 / 180.0 * rect.height(),
                ) * self.analysis.zoom
        };
        for lon in (-180..=180).step_by(30) {
            painter.line_segment(
                [project(lon as f64, -90.0), project(lon as f64, 90.0)],
                egui::Stroke::new(0.5, egui::Color32::DARK_GRAY),
            );
        }
        for lat in (-90..=90).step_by(30) {
            painter.line_segment(
                [project(-180.0, lat as f64), project(180.0, lat as f64)],
                egui::Stroke::new(0.5, egui::Color32::DARK_GRAY),
            );
        }
        for track in &report.tracks.segments {
            for pair in track.samples.windows(2) {
                let (a, b) = (
                    &report.locations[pair[0]].point,
                    &report.locations[pair[1]].point,
                );
                if (a.longitude - b.longitude).abs() <= 180.0 {
                    painter.line_segment(
                        [
                            project(a.longitude, a.latitude),
                            project(b.longitude, b.latitude),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::LIGHT_GREEN),
                    );
                }
            }
        }
        for (i, s) in report.locations.iter().take(5000).enumerate() {
            let point = project(s.point.longitude, s.point.latitude);
            if rect.contains(point) {
                painter.circle_filled(
                    point,
                    if i == self.analysis.detail { 5.0 } else { 3.0 },
                    egui::Color32::LIGHT_BLUE,
                );
                if response.clicked()
                    && response
                        .interact_pointer_pos()
                        .is_some_and(|p| p.distance(point) < 9.0)
                {
                    self.analysis.detail = i;
                }
            }
        }
        ui.label(format!("{} valid locations · {} invalid · {} clusters · {} unresolved track samples / 유효·무효·군집·이동 경로 미확정",report.locations.len(),report.coverage.invalid_locations.len(),report.clusters.len(),report.tracks.unresolved.len()));
        if report.locations.len() > 5000 {
            ui.label("Plot shows first 5,000; list and exports include every location / 도표는 처음 5,000개, 목록·내보내기는 전체 위치를 포함합니다.");
        }
        egui::ScrollArea::vertical()
            .id_salt("locations-list")
            .max_height(180.0)
            .show_rows(ui, 24.0, report.locations.len(), |ui, r| {
                for i in r {
                    let s = &report.locations[i];
                    if ui
                        .selectable_label(
                            self.analysis.detail == i,
                            format!(
                                "{:.6}, {:.6} · {}",
                                s.point.latitude, s.point.longitude, s.path
                            ),
                        )
                        .clicked()
                    {
                        self.analysis.detail = i;
                    }
                }
            });
        if let Some(s) = report.locations.get(self.analysis.detail) {
            json_view(ui, s);
        }
        ui.collapsing(
            "Clusters and track diagnostics / 군집·경로 진단",
            |ui| {
                json_view(ui, &report.clusters);
                json_view(ui, &report.tracks);
                json_view(ui, &report.coverage.invalid_locations);
            },
        );
    }
    pub fn export_geo(&mut self, ctx: &egui::Context, path: PathBuf, kml: bool) {
        let Some(report) = &self.analysis.report else {
            return;
        };
        let samples = report.locations.clone();
        self.task(
            ctx,
            "Exporting locations / 위치 내보내기",
            move || {
                if kml {
                    app::workflows::write_bytes(
                        &path,
                        a::geo::kml(&samples, Default::default())?.as_bytes(),
                    )
                } else {
                    app::workflows::write_json(
                        &path,
                        &a::geo::geojson(&samples, Default::default())?,
                    )
                }
            },
            |_, ()| {},
        );
    }
}
