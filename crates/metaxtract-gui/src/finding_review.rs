use crate::desktop::Desktop;
use eframe::egui;
#[derive(Default)]
pub struct FindingsUi {
    category: String,
    status: String,
    conflict: usize,
}
impl Desktop {
    pub(crate) fn finding_review(&mut self, ui: &mut egui::Ui) {
        let Some(report) = &self.analysis.report else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["All severities", "High", "Medium", "Low", "Information"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.analysis.severity, i, *label);
            }
            egui::ComboBox::from_id_salt("finding-category")
                .selected_text(if self.analysis.findings.category.is_empty() {
                    "All categories"
                } else {
                    &self.analysis.findings.category
                })
                .show_ui(ui, |ui| {
                    for v in [
                        "",
                        "Privacy",
                        "Contradiction",
                        "TamperingIndicator",
                        "AnalysisGap",
                    ] {
                        ui.selectable_value(
                            &mut self.analysis.findings.category,
                            v.into(),
                            if v.is_empty() { "All categories" } else { v },
                        );
                    }
                });
            egui::ComboBox::from_id_salt("finding-status")
                .selected_text(if self.analysis.findings.status.is_empty() {
                    "All extraction statuses"
                } else {
                    &self.analysis.findings.status
                })
                .show_ui(ui, |ui| {
                    for v in ["", "Complete", "Partial", "Failed", "Unsupported"] {
                        ui.selectable_value(
                            &mut self.analysis.findings.status,
                            v.into(),
                            if v.is_empty() {
                                "All extraction statuses"
                            } else {
                                v
                            },
                        );
                    }
                });
        });
        ui.weak("Finding severity, extraction status, and parser diagnostics describe separate aspects of the evidence.");
        let findings: Vec<_> = report
            .records
            .iter()
            .filter(|r| {
                self.analysis.findings.status.is_empty()
                    || r.metadata.get("status").and_then(|s| s.as_str())
                        == Some(&self.analysis.findings.status)
            })
            .flat_map(|r| r.findings.iter().map(move |f| (r, f)))
            .filter(|(_, f)| {
                (self.analysis.findings.category.is_empty()
                    || format!("{:?}", f.category) == self.analysis.findings.category)
                    && (self.analysis.severity == 0
                        || match f.severity {
                            metaxtract_app::analysis::findings::Severity::High => 1,
                            metaxtract_app::analysis::findings::Severity::Medium => 2,
                            metaxtract_app::analysis::findings::Severity::Low => 3,
                            _ => 4,
                        } == self.analysis.severity)
            })
            .collect();
        ui.label(format!("{} findings", findings.len()));
        egui::ScrollArea::vertical()
            .id_salt("findings-review")
            .max_height(200.0)
            .show_rows(ui, 25.0, findings.len(), |ui, range| {
                for i in range {
                    let (_, f) = findings[i];
                    if ui
                        .selectable_label(
                            self.analysis.detail == i,
                            format!(
                                "{:?} · {:?} · {} · {}",
                                f.severity, f.category, f.rule, f.path
                            ),
                        )
                        .clicked()
                    {
                        self.analysis.detail = i;
                    }
                }
            });
        let mut open = None;
        if let Some((r, f)) = findings.get(self.analysis.detail) {
            ui.strong(&f.explanation);
            ui.label(format!(
                "Finding: {:?} · heuristic: {} · extraction: {}",
                f.severity,
                f.heuristic,
                r.metadata
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
            ));
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Open file and inspect evidence"),
                )
                .clicked()
            {
                open = Some((report.scan_id.clone(), f.path.clone()));
            }
            ui.collapsing("Claim and observation evidence", |ui| {
                ui.label(format!(
                    "Claims: {:?} · observations: {:?}",
                    f.claims, f.observations
                ));
                for lineage in &r.lineage {
                    if f.claims.contains(&lineage.claim)
                        || lineage
                            .evidence
                            .iter()
                            .any(|o| f.observations.contains(&o.observation))
                    {
                        ui.strong(format!(
                            "{} · {}",
                            lineage.key,
                            crate::inspectors::claim_brief(&lineage.value)
                        ));
                        for e in &lineage.evidence {
                            ui.label(format!(
                                "#{} · {} · {} · {}",
                                e.observation.get(),
                                e.tag.namespace,
                                e.tag.name.as_deref().unwrap_or("Unnamed tag"),
                                e.provenance.parser.name
                            ));
                        }
                        crate::inspectors::json_view(ui, lineage);
                    }
                }
            });
            ui.collapsing("Parser diagnostics", |ui| {
                if let Some(d) = r.metadata.get("diagnostics") {
                    crate::inspectors::json_view(ui, d);
                }
            });
            ui.collapsing("Normalization and validation issues", |ui| {
                crate::inspectors::json_view(ui, &r.normalization_issues);
                crate::inspectors::json_view(ui, &r.validation_issues);
            });
        }
        ui.separator();
        ui.heading("Conflicting metadata claims");
        let conflicts: Vec<_> = report
            .records
            .iter()
            .filter(|r| {
                r.metadata
                    .get("conflicts")
                    .and_then(|v| v.as_array())
                    .is_some_and(|v| !v.is_empty())
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("conflicting-files")
            .max_height(140.0)
            .show_rows(ui, 24.0, conflicts.len(), |ui, range| {
                for i in range {
                    if ui
                        .selectable_label(
                            self.analysis.findings.conflict == i,
                            conflicts[i].path.to_string(),
                        )
                        .clicked()
                    {
                        self.analysis.findings.conflict = i;
                    }
                }
            });
        if let Some(r) = conflicts.get(self.analysis.findings.conflict) {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Open conflicting file"))
                .clicked()
            {
                open = Some((report.scan_id.clone(), r.path.clone()));
            }
            for lineage in r.lineage.iter().filter(|l| !l.conflicts.is_empty()) {
                ui.label(format!(
                    "{}: {} · {:?}",
                    lineage.key,
                    crate::inspectors::claim_brief(&lineage.value),
                    lineage.resolution
                ));
                crate::inspectors::json_view(ui, &lineage.conflicts);
            }
        }
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
