use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{
        AnalysisLimits,
        relationships::{RelationshipBasis, RelationshipGraph},
    },
};
#[derive(Default)]
pub struct RelationshipsUi {
    graph: Option<RelationshipGraph>,
    scan: String,
    revision: u64,
    total: u64,
    selected: Option<usize>,
    exact: bool,
    zoom: f32,
    pan: egui::Vec2,
}
impl Desktop {
    pub(crate) fn relationships_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(crate::i18n::tr("Cross-file relationships"));
        ui.label(crate::i18n::tr("Blue: recorded digest and size agree. Gold: a shared metadata value; a relationship is unconfirmed. Groups use a spanning star. Shared values do not establish authorship or chronology."));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(crate::i18n::tr(
                    "Build graph from current file filter / selection",
                )),
            )
            .clicked()
        {
            match (self.case.clone(), self.active_query(self.analysis.selected)) {
                (Some(case), Ok(q)) => self.task(
                    ui.ctx(),
                    "Linking file metadata",
                    move || {
                        let revision = case.info()?.revision;
                        let total = case.count_search(&q)?;
                        let page = case.search_records(&q, None, 200)?;
                        let mut records = vec![];
                        for h in page.items {
                            records.push(
                                case.record(h.id)?
                                    .ok_or_else(|| std::io::Error::other("Record disappeared"))?,
                            );
                        }
                        let graph = app::analysis::relationships::build(
                            &records,
                            AnalysisLimits {
                                max_records: 200,
                                max_results: 4000,
                                ..Default::default()
                            },
                        )?;
                        if case.info()?.revision != revision {
                            return Err(std::io::Error::other(
                                "Case changed; rebuild relationships",
                            ));
                        }
                        Ok((graph, q.records.scan_id, revision, total))
                    },
                    |s, (graph, scan, revision, total)| {
                        s.analysis.relationships = RelationshipsUi {
                            graph: Some(graph),
                            scan,
                            revision,
                            total,
                            zoom: 1.0,
                            ..Default::default()
                        };
                    },
                ),
                (_, Err(e)) => self.report_error(e),
                _ => {}
            }
        }
        let state = &mut self.analysis.relationships;
        let Some(graph) = &state.graph else { return };
        ui.label(format!(
            "{} / {} files · {} links · revision {} · {}",
            graph.files.len(),
            state.total,
            graph.edges.len(),
            state.revision,
            state.scan
        ));
        if self
            .info
            .as_ref()
            .is_some_and(|i| i.revision != state.revision)
            || self.active_scan.as_ref() != Some(&state.scan)
        {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Older relationship snapshot; rebuild for current scope.",
            );
        }
        if state.total > 200 {
            ui.label(crate::i18n::tr("Graph is limited to the first 200 files in the current ordering. Narrow the filter for other files."));
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.exact, "Digest links only");
            state.zoom = state.zoom.clamp(0.5, 8.0);
            ui.add(egui::Slider::new(&mut state.zoom, 0.5..=8.0).text(crate::i18n::tr("Zoom")));
            if ui.button(crate::i18n::tr("Fit graph")).clicked() {
                state.zoom = 1.0;
                state.pan = egui::Vec2::ZERO;
            }
        });
        let (r, p) = ui.allocate_painter(
            egui::vec2(ui.available_width().max(200.0), 380.0),
            egui::Sense::click_and_drag(),
        );
        let p = p.with_clip_rect(r.rect);
        if r.dragged() {
            state.pan += ui.input(|i| i.pointer.delta());
        }
        p.rect_filled(r.rect, 4.0, ui.visuals().faint_bg_color);
        let radius = r.rect.width().min(r.rect.height()) * 0.4 * state.zoom;
        let points: Vec<_> = (0..graph.files.len())
            .map(|i| {
                let angle = i as f32 / graph.files.len().max(1) as f32 * std::f32::consts::TAU;
                r.rect.center() + state.pan + egui::vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        for edge in &graph.edges {
            let digest = matches!(edge.basis, RelationshipBasis::ContentDigest { .. });
            if state.exact && !digest {
                continue;
            }
            let color = if digest {
                egui::Color32::LIGHT_BLUE
            } else {
                egui::Color32::GOLD
            };
            let active = state
                .selected
                .is_none_or(|i| i == edge.first || i == edge.second);
            p.line_segment(
                [points[edge.first], points[edge.second]],
                egui::Stroke::new(
                    if active { 1.0 } else { 0.5 },
                    color.gamma_multiply(if active { 0.8 } else { 0.12 }),
                ),
            );
        }
        for (i, point) in points.iter().enumerate() {
            if r.rect.contains(*point) {
                p.circle_filled(
                    *point,
                    if state.selected == Some(i) { 7.0 } else { 4.0 },
                    egui::Color32::LIGHT_GREEN,
                );
                if state.selected == Some(i) || graph.files.len() < 20 {
                    p.text(
                        *point + egui::vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        graph.files[i]
                            .to_string()
                            .chars()
                            .take(30)
                            .collect::<String>(),
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                }
                if r.clicked()
                    && r.interact_pointer_pos()
                        .is_some_and(|v| v.distance(*point) < 10.0)
                {
                    state.selected = Some(i);
                }
            }
        }
        let mut open = None;
        egui::ComboBox::from_id_salt("relationship-file")
            .selected_text(
                state
                    .selected
                    .and_then(|i| graph.files.get(i))
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "Select a file node".into()),
            )
            .show_ui(ui, |ui| {
                for (i, path) in graph.files.iter().enumerate() {
                    ui.selectable_value(&mut state.selected, Some(i), path.to_string());
                }
            });
        if let Some(i) = state.selected {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new(crate::i18n::tr("Open selected file")),
                )
                .clicked()
            {
                open = Some((state.scan.clone(), graph.files[i].clone()));
            }
            for edge in graph.edges.iter().filter(|e| e.first == i || e.second == i) {
                if state.exact && !matches!(edge.basis, RelationshipBasis::ContentDigest { .. }) {
                    continue;
                }
                let other = if edge.first == i {
                    edge.second
                } else {
                    edge.first
                };
                ui.collapsing(graph.files[other].to_string(), |ui| {
                    match &edge.basis {
                        RelationshipBasis::ContentDigest { algorithm, digest } => {
                            ui.label(format!("Recorded {algorithm:?}: {digest}"));
                        }
                        RelationshipBasis::SharedMetadata { key, value } => {
                            ui.label(format!(
                                "Shared claim: {key} = {}",
                                crate::inspectors::claim_brief(value)
                            ));
                            ui.weak(crate::i18n::tr("Unconfirmed relationship"));
                        }
                    }
                    if ui.button(crate::i18n::tr("Select neighbor")).clicked() {
                        state.selected = Some(other);
                    }
                });
            }
        }
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
