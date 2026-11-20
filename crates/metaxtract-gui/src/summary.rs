use eframe::egui;
use metaxtract_app::schema::*;

#[derive(Default)]
pub struct Summary {
    rows: Vec<(&'static str, String, String)>,
}
impl Summary {
    pub fn new(record: &ExtractionRecord) -> Self {
        let mut rows = Vec::new();
        let mut shown = std::collections::BTreeSet::new();
        for claim in &record.claims {
            if claim.resolution == ClaimResolution::Rejected {
                continue;
            }
            let key = claim.key.as_str();
            if key.starts_with("raw.") {
                continue;
            }
            let name = if key.contains("captured") {
                Some("Captured")
            } else if key.contains("location.gps") {
                Some("Location")
            } else if key.contains("title") {
                Some("Title")
            } else if key.contains("author") {
                Some("Author")
            } else if key.ends_with("metadata.device_make") || key.contains("camera.make") {
                Some("Camera make")
            } else if key.ends_with("metadata.device_model") || key.contains("camera.model") {
                Some("Camera model")
            } else if key.ends_with("width") {
                Some("Width")
            } else if key.ends_with("height") {
                Some("Height")
            } else if key.contains("duration") {
                Some("Duration")
            } else {
                None
            };
            if let Some(name) = name {
                if matches!(name, "Width" | "Height")
                    && match claim.value {
                        NormalizedValue::Signed(v) => v <= 0,
                        NormalizedValue::Unsigned(v) => v == 0,
                        NormalizedValue::Number(v) => v <= 0.0 || !v.is_finite(),
                        _ => false,
                    }
                {
                    continue;
                }
                let signature = format!("{name}:{}", crate::inspectors::claim_brief(&claim.value));
                if shown.insert(signature) {
                    rows.push((
                        name,
                        crate::inspectors::claim_brief(&claim.value),
                        key.to_owned(),
                    ));
                    if rows.len() >= 16 {
                        break;
                    }
                }
            }
        }
        Self { rows }
    }
}
pub fn overview(ui: &mut egui::Ui, record: &ExtractionRecord, summary: &Summary) {
    let path = record.source.path.to_string();
    ui.heading(path.rsplit('/').next().unwrap_or(&path));
    ui.add(egui::Label::new(&path).selectable(true).wrap());
    egui::Grid::new("metadata-summary")
        .striped(true)
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in [
                (
                    "Type",
                    record
                        .source
                        .detected_media_type
                        .as_ref()
                        .or(record.source.declared_media_type.as_ref())
                        .cloned()
                        .unwrap_or_else(|| "—".into()),
                ),
                (
                    "Size",
                    crate::formatting::bytes(record.source.identity.size_bytes),
                ),
                (
                    "Status",
                    crate::i18n::tr(&format!("{:?}", record.status)).to_owned(),
                ),
                (
                    "Modified (UTC)",
                    crate::formatting::modified(
                        record
                            .source
                            .identity
                            .modified_ns
                            .map(|n| n.to_string())
                            .as_deref(),
                    ),
                ),
            ] {
                ui.label(crate::i18n::tr(label));
                ui.add(egui::Label::new(value).selectable(true).wrap());
                ui.end_row();
            }
            for (name, value, key) in &summary.rows {
                ui.label(crate::i18n::tr(name));
                ui.add(egui::Label::new(value).selectable(true).wrap())
                    .on_hover_text(key);
                ui.end_row();
            }
        });
    ui.collapsing("SHA-256", |ui| {
        for digest in &record.digests {
            if digest.algorithm == DigestAlgorithm::Sha256 {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            digest
                                .bytes
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect::<String>(),
                        )
                        .monospace(),
                    )
                    .selectable(true)
                    .wrap(),
                );
            }
        }
    });
    ui.small(crate::i18n::tr("Missing metadata is not inferred. Conflicting values remain available with their evidence."));
}
