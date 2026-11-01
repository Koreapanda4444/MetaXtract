use eframe::egui;
use metaxtract_app::{analysis::provenance::FieldLineage, schema::ObservationId};
pub fn graph(ui: &mut egui::Ui, field: &FieldLineage) -> Option<ObservationId> {
    ui.strong(format!("{} · claim #{}", field.key, field.claim.get()));
    ui.add(
        egui::Label::new(crate::inspectors::claim_brief(&field.value))
            .selectable(true)
            .wrap(),
    );
    ui.label(format!(
        "{:?} · confidence {}‰ · {:?}",
        field.resolution,
        field.confidence.score_millis(),
        field.agreement
    ));
    ui.weak(crate::i18n::tr("Parser → original tag / raw observation → normalized claim. Red links contradict the claim. Click a raw node to inspect its original value."));
    if field.evidence.is_empty() {
        ui.label(crate::i18n::tr(
            "No raw observation links were retained for this claim.",
        ));
        return None;
    }
    let mut selected = None;
    egui::ScrollArea::both()
        .id_salt("provenance-graph-scroll")
        .max_height(360.0)
        .show(ui, |ui| {
            let n = field.evidence.len().min(32);
            let (response, p) = ui.allocate_painter(
                egui::vec2(660.0, n as f32 * 68.0 + 24.0),
                egui::Sense::hover(),
            );
            let origin = response.rect.min;
            let claim = egui::Rect::from_center_size(
                origin + egui::vec2(565.0, (n as f32 * 68.0 + 24.0) / 2.0),
                egui::vec2(170.0, 52.0),
            );
            p.rect_filled(claim, 5.0, egui::Color32::from_rgb(30, 80, 105));
            p.text(
                claim.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}\nclaim #{}", field.key, field.claim.get()),
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
            for (i, e) in field.evidence.iter().take(32).enumerate() {
                let y = origin.y + 34.0 + i as f32 * 68.0;
                let parser = egui::Rect::from_center_size(
                    egui::pos2(origin.x + 90.0, y),
                    egui::vec2(170.0, 52.0),
                );
                let raw = egui::Rect::from_center_size(
                    egui::pos2(origin.x + 320.0, y),
                    egui::vec2(200.0, 52.0),
                );
                let color = if e.contradicts {
                    egui::Color32::LIGHT_RED
                } else {
                    egui::Color32::LIGHT_GREEN
                };
                p.arrow(
                    parser.right_center(),
                    raw.left_center() - parser.right_center(),
                    egui::Stroke::new(1.0, egui::Color32::GRAY),
                );
                p.arrow(
                    raw.right_center(),
                    claim.left_center() - raw.right_center(),
                    egui::Stroke::new(1.0, color),
                );
                p.rect_filled(parser, 4.0, egui::Color32::from_rgb(45, 48, 58));
                p.rect_filled(raw, 4.0, egui::Color32::from_rgb(40, 62, 55));
                let short = |s: &str| s.chars().take(25).collect::<String>();
                p.text(
                    parser.center(),
                    egui::Align2::CENTER_CENTER,
                    format!(
                        "{}\n{}",
                        short(&e.provenance.parser.name),
                        short(&e.provenance.parser.version)
                    ),
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
                p.text(
                    raw.center(),
                    egui::Align2::CENTER_CENTER,
                    format!(
                        "{} · #{}\n{}",
                        short(&e.tag.namespace),
                        e.observation.get(),
                        short(e.tag.name.as_deref().unwrap_or("Unnamed tag"))
                    ),
                    egui::FontId::proportional(12.0),
                    color,
                );
                let click = ui
                    .interact(raw, ui.id().with(("raw-node", i)), egui::Sense::click())
                    .on_hover_text(format!(
                        "{} · {:?} · {:?}\n{:?}",
                        field.path, e.tag.code, e.provenance.source_range, e.provenance.method
                    ));
                if click.clicked() {
                    selected = Some(e.observation);
                }
            }
        });
    if field.evidence.len() > 32 {
        ui.label(format!(
            "Showing 32 / {} links; complete evidence remains below.",
            field.evidence.len()
        ));
    }
    ui.collapsing(
        crate::i18n::tr("Full provenance, ranges and conflicts"),
        |ui| crate::inspectors::json_view(ui, field),
    );
    selected
}
