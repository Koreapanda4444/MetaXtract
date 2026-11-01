use crate::desktop::Desktop;
use eframe::egui;
pub(crate) fn bar(
    ui: &mut egui::Ui,
    label: &str,
    value: u64,
    max: u64,
    color: egui::Color32,
) -> egui::Response {
    let (r, p) = ui.allocate_painter(
        egui::vec2(ui.available_width().clamp(100.0, 700.0), 28.0),
        egui::Sense::click(),
    );
    p.rect_filled(r.rect, 3.0, ui.visuals().faint_bg_color);
    let fill = egui::Rect::from_min_size(
        r.rect.min,
        egui::vec2(
            r.rect.width() * value as f32 / max.max(1) as f32,
            r.rect.height(),
        ),
    );
    p.rect_filled(fill, 3.0, color.gamma_multiply(0.35));
    p.text(
        r.rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{label} · {value}"),
        egui::FontId::proportional(14.0),
        ui.visuals().text_color(),
    );
    r
}
impl Desktop {
    pub(crate) fn dashboard(&mut self, ui: &mut egui::Ui) {
        let mut mime = None;
        let mut status = None;
        ui.heading(crate::i18n::tr("File distribution · current file filter"));
        let max = self
            .analysis
            .mime_facets
            .iter()
            .map(|v| v.records)
            .max()
            .unwrap_or(1);
        for facet in &self.analysis.mime_facets {
            if bar(
                ui,
                &facet.value,
                facet.records,
                max,
                egui::Color32::LIGHT_BLUE,
            )
            .clicked()
            {
                mime = Some(facet.value.clone());
            }
        }
        if let Some(report) = &self.analysis.report {
            ui.horizontal_wrapped(|ui| {
                for (label, value) in [
                    ("Selected", report.coverage.selected_records),
                    ("Excluded", report.coverage.excluded_records),
                    (
                        "Exact duplicate groups",
                        report.exact_duplicates.len() as u64,
                    ),
                    ("Location samples", report.locations.len() as u64),
                ] {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.strong(value.to_string());
                            ui.label(label);
                        });
                    });
                }
            });
            ui.heading(crate::i18n::tr("Extraction status · analysis snapshot"));
            let max = report
                .coverage
                .status_counts
                .values()
                .copied()
                .max()
                .unwrap_or(1);
            for (key, value) in &report.coverage.status_counts {
                if bar(ui, key, *value, max, egui::Color32::LIGHT_GREEN).clicked() {
                    status = serde_json::from_value(serde_json::Value::String(key.clone())).ok();
                }
            }
            ui.heading(crate::i18n::tr("Findings · analysis snapshot"));
            let max = report
                .coverage
                .findings_by_severity
                .values()
                .copied()
                .max()
                .unwrap_or(1);
            for (key, value) in &report.coverage.findings_by_severity {
                if bar(ui, key, *value, max, egui::Color32::LIGHT_RED).clicked() {
                    self.analysis.view = 4;
                }
            }
            ui.collapsing(crate::i18n::tr("Coverage and analysis limits"), |ui| {
                crate::inspectors::json_view(ui, &report.coverage)
            });
        }
        if (mime.is_some() || status.is_some()) && !self.busy {
            if let Ok(mut q) = self.active_query(false) {
                if let Some(m) = mime {
                    q.records.mime = Some(m);
                }
                if let Some(s) = status {
                    q.records.status = Some(s);
                }
                self.apply_search(q);
            }
        }
    }
}
