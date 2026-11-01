use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{
        diff::FieldSegment,
        evolution::{Evolution, ScanSnapshot},
    },
    storage::{
        SearchExpression as E, SearchField as F, SearchOperator as O, SearchPredicate, SearchQuery,
    },
};
use std::collections::BTreeSet;
#[derive(Default)]
pub struct EvolutionUi {
    path: String,
    scans: BTreeSet<String>,
    history: Option<Evolution>,
    revision: u64,
    selected: usize,
    filter: String,
}
fn field_name(path: &[FieldSegment]) -> String {
    path.iter()
        .map(|v| match v {
            FieldSegment::Key(k) => format!(".{k}"),
            FieldSegment::Index(i) => format!("[{i}]"),
        })
        .collect::<String>()
        .trim_start_matches('.')
        .into()
}
impl Desktop {
    pub(crate) fn evolution_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(crate::i18n::tr("Metadata changes across scans"));
        ui.label(crate::i18n::tr("Track one evidence path across 2–16 chosen scans. Dates show scan start time, not the time metadata was edited. Missing records do not prove deletion; identical paths alone do not establish file identity."));
        let state = &mut self.analysis.evolution;
        if state.path.is_empty() {
            if let Some((_, r)) = &self.records.focused {
                state.path = r.source.path.to_string();
            }
        }
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Evidence path"));
            ui.add(
                egui::TextEdit::singleline(&mut state.path)
                    .desired_width(400.0)
                    .char_limit(4096),
            );
        });
        egui::CollapsingHeader::new(crate::textf!(
            "Choose scans · {} selected",
            "스캔 선택 · {}개 선택",
            state.scans.len()
        ))
        .id_salt("history-scan-selector")
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("history-scans")
                .max_height(180.0)
                .show(ui, |ui| {
                    for scan in &self.scans {
                        let mut selected = state.scans.contains(&scan.id);
                        if ui
                            .checkbox(
                                &mut selected,
                                format!(
                                    "{} · {} · {} · {:?}",
                                    crate::formatting::unix_seconds(
                                        i128::from(scan.started_ms) / 1000
                                    ),
                                    scan.source,
                                    scan.record_count,
                                    scan.status
                                ),
                            )
                            .changed()
                        {
                            if selected {
                                state.scans.insert(scan.id.clone());
                            } else {
                                state.scans.remove(&scan.id);
                            }
                        }
                    }
                });
        });
        if ui
            .add_enabled(
                !self.busy
                    && (2..=16).contains(&state.scans.len())
                    && !state.path.trim().is_empty(),
                egui::Button::new(crate::i18n::tr("Build field history")),
            )
            .clicked()
        {
            let path = state.path.clone();
            let scans: Vec<_> = self
                .scans
                .iter()
                .filter(|s| state.scans.contains(&s.id))
                .cloned()
                .collect();
            if let Some(case) = self.case.clone() {
                self.task(
                    ui.ctx(),
                    "Comparing metadata history",
                    move || {
                        let revision = case.info()?.revision;
                        let mut snapshots = vec![];
                        let mut retained=0usize;
                        let mut evidence_path = None;
                        for scan in scans {
                            let q = SearchQuery::new(
                                &scan.id,
                                E::Predicate(SearchPredicate {
                                    field: F::Path,
                                    operator: O::Equal,
                                    value: path.clone(),
                                }),
                            );
                            let page = case.search_records(&q, None, 2)?;
                            if page.items.len() > 1 {
                                return Err(std::io::Error::other("Ambiguous path in scan"));
                            }
                            let record = page
                                .items
                                .first()
                                .map(|h| case.record(h.id))
                                .transpose()?
                                .flatten();
                            if let Some(r) = &record {
                                retained=retained.saturating_add(serde_json::to_vec(r).map_err(std::io::Error::other)?.len());
                                if retained>app::analysis::AnalysisLimits::default().max_bytes{return Err(std::io::Error::other("History input exceeds the memory budget; choose fewer scans"));}
                                evidence_path = Some(r.source.path.clone());
                            }
                            snapshots.push(ScanSnapshot {
                                scan_id: scan.id,
                                started_ms: scan.started_ms,
                                record,
                            });
                        }
                        let path = evidence_path.ok_or_else(|| {
                            std::io::Error::other("Evidence path is absent from every chosen scan")
                        })?;
                        let history = app::analysis::evolution::history(
                            &path,
                            snapshots,
                            app::analysis::AnalysisLimits {
                                max_records: 16,
                                ..Default::default()
                            },
                        )?;
                        if case.info()?.revision != revision {
                            return Err(std::io::Error::other(
                                "Case changed while building history; retry",
                            ));
                        }
                        Ok((history, revision))
                    },
                    |s, (history, revision)| {
                        s.analysis.evolution.history = Some(history);
                        s.analysis.evolution.revision = revision;
                        s.analysis.evolution.selected = 0;
                    },
                );
            }
        }
        let state = &mut self.analysis.evolution;
        let Some(history) = &state.history else {
            return;
        };
        ui.label(format!("{} · revision {}", history.path, state.revision));
        if self
            .info
            .as_ref()
            .is_some_and(|i| i.revision != state.revision)
        {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Older history snapshot; rebuild after case changes.",
            );
        }
        let (r, p) = ui.allocate_painter(
            egui::vec2(ui.available_width().max(200.0), 105.0),
            egui::Sense::click(),
        );
        for (i, scan) in history.scans.iter().enumerate() {
            let x = r.rect.left()
                + 20.0
                + (r.rect.width() - 40.0) * i as f32 / (history.scans.len() - 1) as f32;
            let point = egui::pos2(x, r.rect.center().y - 8.0);
            if i > 0 {
                let previous = egui::pos2(
                    r.rect.left()
                        + 20.0
                        + (r.rect.width() - 40.0) * (i - 1) as f32
                            / (history.scans.len() - 1) as f32,
                    point.y,
                );
                let t = &history.transitions[i - 1];
                let changed = t
                    .changes
                    .as_ref()
                    .is_some_and(|d| !d.fields.is_empty() || !d.claims.is_empty());
                p.line_segment(
                    [previous, point],
                    egui::Stroke::new(
                        if state.selected == i - 1 { 4.0 } else { 2.0 },
                        if changed {
                            egui::Color32::GOLD
                        } else {
                            egui::Color32::GRAY
                        },
                    ),
                );
            }
            p.circle_filled(
                point,
                6.0,
                if scan.present {
                    egui::Color32::LIGHT_GREEN
                } else {
                    egui::Color32::LIGHT_RED
                },
            );
            p.text(
                point + egui::vec2(0.0, 18.0),
                if i == 0 {
                    egui::Align2::LEFT_TOP
                } else if i + 1 == history.scans.len() {
                    egui::Align2::RIGHT_TOP
                } else {
                    egui::Align2::CENTER_TOP
                },
                format!(
                    "{}\n{}",
                    i + 1,
                    crate::formatting::unix_seconds(i128::from(scan.started_ms) / 1000)
                        .chars()
                        .take(10)
                        .collect::<String>()
                ),
                egui::FontId::proportional(11.0),
                ui.visuals().text_color(),
            );
            if r.clicked()
                && r.interact_pointer_pos()
                    .is_some_and(|v| v.distance(point) < 15.0)
            {
                state.selected = i.saturating_sub(1);
            }
        }
        ui.weak(crate::i18n::tr("Scans are ordered chronologically with equal spacing. Green: record present; red: absent; gold: metadata changed."));
        let mut open = None;
        for (i, t) in history.transitions.iter().enumerate() {
            if ui
                .selectable_label(
                    state.selected == i,
                    format!(
                        "{} → {} · {:?} · {} raw changes · {} claim changes",
                        i + 1,
                        i + 2,
                        t.presence,
                        t.changes.as_ref().map(|d| d.fields.len()).unwrap_or(0),
                        t.changes.as_ref().map(|d| d.claims.len()).unwrap_or(0)
                    ),
                )
                .clicked()
            {
                state.selected = i;
            }
        }
        if let Some(t) = history.transitions.get(state.selected) {
            ui.horizontal_wrapped(|ui| {
                for (label, index) in [("Open before", t.before), ("Open after", t.after)] {
                    let scan = &history.scans[index];
                    if ui
                        .add_enabled(!self.busy && scan.present, egui::Button::new(label))
                        .clicked()
                    {
                        open = Some((scan.scan_id.clone(), history.path.clone()));
                    }
                    ui.label(format!(
                        "{} · {}",
                        scan.scan_id,
                        crate::formatting::unix_seconds(i128::from(scan.started_ms) / 1000)
                    ));
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .hint_text(crate::i18n::tr("Filter field names"))
                    .char_limit(256),
            );
            if let Some(d) = &t.changes {
                ui.label(format!("Content changed: {:?}", d.content_changed));
                for field in d
                    .fields
                    .iter()
                    .filter(|f| field_name(&f.field).contains(&state.filter))
                    .take(200)
                {
                    ui.collapsing(field_name(&field.field), |ui| {
                        ui.columns(2, |c| {
                            c[0].colored_label(egui::Color32::LIGHT_RED, "Before");
                            crate::inspectors::json_view(&mut c[0], &field.before);
                            c[1].colored_label(egui::Color32::LIGHT_GREEN, "After");
                            crate::inspectors::json_view(&mut c[1], &field.after);
                        });
                    });
                }
                for claim in d
                    .claims
                    .iter()
                    .filter(|c| c.key.as_str().contains(&state.filter))
                    .take(200)
                {
                    ui.collapsing(
                        format!(
                            "Claim {} · semantically equal: {}",
                            claim.key, claim.semantically_equal
                        ),
                        |ui| {
                            crate::inspectors::json_view(ui, claim);
                        },
                    );
                }
                ui.weak(crate::i18n::tr(
                    "Up to 200 matching raw fields and 200 claim changes are displayed.",
                ));
            } else {
                ui.label(crate::i18n::tr("A record is missing at one or both endpoints; no field comparison is inferred."));
            }
        }
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
