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
    pub(crate) evolution: crate::evolution::EvolutionUi,
    pub(crate) relationships: crate::relationships::RelationshipsUi,
    pub(crate) findings: crate::finding_review::FindingsUi,
    pub(crate) pair: crate::duplicate_review::PairUi,
    pub(crate) timeline: crate::timeline::TimelineUi,
    pub(crate) mime_facets: Vec<metaxtract_app::storage::FacetValue>,
    pub(crate) totals: Option<CaseTotals>,
    pub(crate) revision: u64,
    pub(crate) scan_id: Option<String>,
    pub(crate) statistics: Option<serde_json::Value>,
    pub metrics_loaded: bool,
    pub(crate) report: Option<Box<InvestigationReport>>,
    pub(crate) selected: bool,
    pub(crate) view: usize,
    pub(crate) detail: usize,
    pub(crate) severity: usize,
    pub(crate) radius: f64,
    pub(crate) groups: std::collections::BTreeMap<String, String>,
    pub(crate) show_tracks: bool,
    pub(crate) show_clusters: bool,
    pub(crate) cluster: Option<usize>,
    pub(crate) zoom: f32,
    pub(crate) center: egui::Vec2,
}
impl Default for AnalysisUi {
    fn default() -> Self {
        Self {
            evolution: Default::default(),
            relationships: Default::default(),
            findings: Default::default(),
            pair: Default::default(),
            timeline: Default::default(),
            mime_facets: vec![],
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
            groups: Default::default(),
            show_tracks: true,
            show_clusters: true,
            cluster: None,
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
            .ok_or_else(|| io::Error::other(crate::i18n::tr("Select a scan")))?;
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
        let query = self.active_query(false).ok();
        self.analysis.metrics_loaded = true;
        self.analysis.scan_id = scan.clone();
        self.task(
            ctx,
            crate::i18n::tr("Loading case totals"),
            move || {
                let before = case.info()?.revision;
                let totals = case.totals()?;
                let statistics = scan
                    .as_deref()
                    .map(|s| case.scan_receipt(s, "application.summary"))
                    .transpose()?
                    .flatten();
                if before != case.info()?.revision {
                    return Err(io::Error::other(crate::i18n::tr(
                        "Case changed; refresh statistics",
                    )));
                }
                let facets = if let Some(q) = query {
                    case.search_facets(&q, metaxtract_app::storage::FacetField::Mime, 32)?
                        .values
                } else {
                    vec![]
                };
                Ok((totals, before, statistics, facets))
            },
            |s, (totals, revision, statistics, facets)| {
                s.analysis.mime_facets = facets;
                s.analysis.totals = Some(totals);
                s.analysis.revision = revision;
                s.analysis.statistics = statistics;
            },
        );
    }
    pub fn analysis_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, crate::i18n::tr("Case analysis"));
        if self.case.is_none() {
            ui.label(crate::i18n::tr("Open a case first"));
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    crate::components::button(crate::i18n::tr("Refresh totals")),
                )
                .clicked()
            {
                self.analysis.metrics_loaded = false;
            }
            ui.checkbox(
                &mut self.analysis.selected,
                crate::i18n::tr("Selected records only"),
            );
            if ui
                .add_enabled(
                    !self.busy && !self.scan.running(),
                    crate::components::button(crate::i18n::tr("Build analysis")),
                )
                .clicked()
            {
                let result = (|| -> io::Result<_> {
                    let query = self.active_query(self.analysis.selected)?;
                    let mut options =
                        app::workflows::report_options(crate::i18n::tr("Case analysis"), "")?;
                    options.cluster_radius_meters = self.analysis.radius;
                    options.track_groups = self.analysis.groups.clone();
                    Ok((query, options))
                })();
                match result {
                    Ok((query, options)) => {
                        let case = self.case.clone().expect("open case");
                        self.task(
                            ui.ctx(),
                            crate::i18n::tr("Analyzing metadata"),
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
                                s.analysis.pair = Default::default();
                                s.analysis.report = Some(r);
                                s.analysis.detail = 0;
                            },
                        );
                    }
                    Err(e) => self.report_error(e),
                }
            }
        });
        ui.label(crate::i18n::tr("Scope uses the current record filter or selected IDs. Analysis is a revision-bound snapshot."));
        ui.collapsing(crate::i18n::tr("Geographic options"),|ui| {
            ui.horizontal(|ui| { ui.label(crate::i18n::tr("Cluster radius (m)")); ui.add(egui::DragValue::new(&mut self.analysis.radius).range(0.0..=1_000_000.0)); });
            ui.label(crate::i18n::tr("Assign paths to track groups, then rebuild analysis. Unassigned locations remain unresolved."));
            if let Some(report)=&self.analysis.report {
                let paths:std::collections::BTreeSet<_>=report.locations.iter().map(|p|p.path.to_string()).collect();
                let paths:Vec<_>=paths.into_iter().collect();
                egui::ScrollArea::vertical().id_salt("track-group-form").max_height(180.0).show_rows(ui,30.0,paths.len(),|ui,range| {
                    for i in range {ui.horizontal(|ui| {ui.label(&paths[i]);ui.add(egui::TextEdit::singleline(self.analysis.groups.entry(paths[i].clone()).or_default()).desired_width(160.0).char_limit(128).hint_text(crate::i18n::tr("Group name")));});}
                });
                self.analysis.groups.retain(|_,v|!v.trim().is_empty());
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (i, label) in [
                crate::i18n::tr("Dashboard"),
                crate::i18n::tr("Timeline"),
                crate::i18n::tr("Locations"),
                crate::i18n::tr("Duplicates"),
                crate::i18n::tr("Findings"),
                crate::i18n::tr("Relationships"),
                crate::i18n::tr("Changes across scans"),
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
                ui.label(crate::textf!(
                    "Whole case · revision {}",
                    "전체 케이스 · 수정 {}",
                    self.analysis.revision
                ));
                ui.horizontal_wrapped(|ui| {
                    for (label, v) in [
                        (crate::i18n::tr("Scans"), t.scans),
                        (crate::i18n::tr("Records"), t.records),
                        (crate::i18n::tr("Raw fields"), t.observations),
                        (crate::i18n::tr("Claims"), t.claims),
                        (crate::i18n::tr("Conflicts"), t.conflicts),
                        (crate::i18n::tr("Diagnostics"), t.diagnostics),
                    ] {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.strong(v.to_string());
                                ui.label(label);
                            });
                        });
                    }
                });
                ui.collapsing(crate::i18n::tr("Persisted file statistics"), |ui| {
                    if let Some(s) = &self.analysis.statistics {
                        json_view(ui, s);
                    } else {
                        ui.label(crate::i18n::tr("Unavailable for older/imported scans"));
                    }
                });
            }
        }
        if self.analysis.view == 6 {
            self.evolution_view(ui);
            return;
        }
        if self.analysis.view == 5 {
            self.relationships_view(ui);
            return;
        }
        let Some(report) = self.analysis.report.as_ref() else {
            ui.label(crate::i18n::tr("Run analysis to populate these views"));
            return;
        };
        ui.label(format!(
            "Snapshot: {} · revision {} · {} / {} records",
            report.scan_id,
            report.case_revision,
            report.coverage.selected_records,
            report.coverage.full_scan_records
        ));
        let scope_changed = self
            .active_query(self.analysis.selected)
            .ok()
            .zip(report.query.as_ref())
            .is_some_and(|(a, b)| serde_json::to_string(&a).ok() != serde_json::to_string(b).ok());
        if scope_changed
            || self.active_scan.as_ref() != Some(&report.scan_id)
            || self
                .info
                .as_ref()
                .is_some_and(|i| i.revision != report.case_revision)
        {
            ui.colored_label(
                egui::Color32::YELLOW,
                crate::i18n::tr("Older snapshot; run analysis again for the current selection"),
            );
        }
        match self.analysis.view {
            0 => {
                self.dashboard(ui);
                for text in &self.analysis.report.as_ref().expect("report").limitations {
                    ui.label(text);
                }
            }
            1 => self.timeline_view(ui),
            2 => self.location_view(ui),
            3 => self.duplicate_review(ui),
            _ => self.finding_review(ui),
        }
    }

    fn location_view(&mut self, ui: &mut egui::Ui) {
        let report = self.analysis.report.as_ref().expect("analysis snapshot");
        ui.label(crate::i18n::tr("Offline coordinate plot · longitude"));
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.analysis.zoom, 1.0..=64.0)
                    .logarithmic(true)
                    .text(crate::i18n::tr("Zoom")),
            );
            if ui.button(crate::i18n::tr("Reset")).clicked() {
                self.analysis.zoom = 1.0;
                self.analysis.center = egui::Vec2::ZERO;
            }
            for (label, p) in [("GeoJSON", Purpose::GeoJson), ("KML", Purpose::Kml)] {
                if ui
                    .add_enabled(!self.busy, crate::components::button(label))
                    .clicked()
                {
                    self.picker = Some(Picker::new(p));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.analysis.show_tracks, "Tracks");
            ui.checkbox(&mut self.analysis.show_clusters, "Clusters");
        });
        let plot_width = ui.available_width().max(100.0);
        egui::ComboBox::from_id_salt("geo-cluster")
            .selected_text(
                self.analysis
                    .cluster
                    .map(|i| format!("Cluster {}", i + 1))
                    .unwrap_or_else(|| "All clusters".into()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.analysis.cluster, None, "All clusters");
                for (i, c) in report.clusters.iter().enumerate() {
                    if ui
                        .selectable_value(
                            &mut self.analysis.cluster,
                            Some(i),
                            format!("Cluster {} · {} points", i + 1, c.members.len()),
                        )
                        .clicked()
                    {
                        if let Some(center) = &c.centroid {
                            let width = plot_width;
                            let extent = c
                                .members
                                .iter()
                                .filter_map(|j| report.locations.get(*j))
                                .fold(0.01_f64, |v, p| {
                                    v.max((p.point.longitude - center.longitude).abs())
                                        .max((p.point.latitude - center.latitude).abs() * 2.0)
                                });
                            self.analysis.zoom = (90.0 / extent).clamp(1.0, 64.0) as f32;
                            self.analysis.center = egui::vec2(
                                -center.longitude as f32 / 360.0 * width,
                                center.latitude as f32 / 180.0 * 250.0,
                            ) * self.analysis.zoom;
                            if let Some(j) = c.members.first() {
                                self.analysis.detail = *j;
                            }
                        }
                    }
                }
            });
        let (response, painter) = ui.allocate_painter(
            egui::vec2(ui.available_width().max(100.0), 250.0),
            egui::Sense::click_and_drag(),
        );
        let rect = response.rect;
        let painter = painter.with_clip_rect(rect);
        if response.dragged() {
            self.analysis.center += ui.input(|i| i.pointer.delta());
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
        for track in report
            .tracks
            .segments
            .iter()
            .filter(|_| self.analysis.show_tracks)
        {
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
        if self.analysis.show_clusters {
            for (i, cluster) in report.clusters.iter().take(5000).enumerate() {
                if let Some(c) = &cluster.centroid {
                    let point = project(c.longitude, c.latitude);
                    if rect.contains(point) {
                        painter.circle_stroke(
                            point,
                            12.0,
                            egui::Stroke::new(1.5, egui::Color32::GOLD),
                        );
                        painter.text(
                            point + egui::vec2(13.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("C{} ({})", i + 1, cluster.members.len()),
                            egui::FontId::proportional(11.0),
                            egui::Color32::GOLD,
                        );
                    }
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
        ui.label(crate::textf!(
            "{} valid locations · {} invalid · {} clusters · {} unresolved track samples",
            "유효 위치 {} · 무효 {} · 군집 {} · 이동 경로 미확정 {}",
            report.locations.len(),
            report.coverage.invalid_locations.len(),
            report.clusters.len(),
            report.tracks.unresolved.len()
        ));
        if report.locations.len() > 5000 {
            ui.label(crate::i18n::tr(
                "Plot shows first 5,000; list and exports include every location",
            ));
        }
        egui::ScrollArea::vertical()
            .id_salt("locations-list")
            .max_height(180.0)
            .show_rows(ui, 30.0, report.locations.len(), |ui, r| {
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
        let mut open = None;
        if let Some(s) = report.locations.get(self.analysis.detail) {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(crate::i18n::tr("Open location file")),
                )
                .clicked()
            {
                open = Some((report.scan_id.clone(), s.path.clone()));
            }
            json_view(ui, s);
        }
        ui.collapsing(crate::i18n::tr("Clusters and track diagnostics"), |ui| {
            json_view(ui, &report.clusters);
            json_view(ui, &report.tracks);
            json_view(ui, &report.coverage.invalid_locations);
        });
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
    pub fn export_geo(&mut self, ctx: &egui::Context, path: PathBuf, kml: bool) {
        let Some(report) = &self.analysis.report else {
            return;
        };
        let samples = report.locations.clone();
        self.task(
            ctx,
            crate::i18n::tr("Exporting locations"),
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
