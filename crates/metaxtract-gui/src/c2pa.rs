use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{
        AnalysisLimits,
        c2pa::{C2paEvidence, C2paValue},
    },
};
#[derive(Default)]
pub struct C2paUi {
    reports: Vec<C2paEvidence>,
    scan: String,
    revision: u64,
    total: u64,
    loaded: bool,
}
fn brief(value: &C2paValue) -> String {
    match value {
        C2paValue::Text(v) | C2paValue::Integer(v) | C2paValue::JsonText(v) => {
            v.chars().take(256).collect()
        }
        C2paValue::Bytes(v) => format!(
            "{}{}",
            v.chars().take(128).collect::<String>(),
            if v.len() > 128 { "…" } else { "" }
        ),
        C2paValue::FloatBits(v) => format!("{} (0x{v:016x})", f64::from_bits(*v)),
        C2paValue::Bool(v) => v.to_string(),
        C2paValue::Null => "null".into(),
        C2paValue::Array(v) => format!("[{}]", v.len()),
        C2paValue::Map(v) => format!("{{{}}}", v.len()),
        C2paValue::Tag { number, .. } => format!("tag {number}"),
    }
}
fn verdict(value: &str) -> &'static str {
    crate::i18n::tr(match value {
        "valid_for_claim_bytes" => "Claim signature matches the original claim bytes",
        "valid_referenced_assertion_hashes" => "Referenced assertion hashes agree",
        "valid_for_supported_asset_ranges" => "File data hash agrees with the supported ranges",
        "invalid" => "Integrity check failed",
        "resource_limit" => "Verification reached a resource limit",
        "not_current_asset_manifest" => {
            "Ingredient or earlier manifest; current file binding is not evaluated"
        }
        "external_asset_required" => "The original asset is required for file binding",
        status if status.starts_with("unsupported") => "Unsupported verification scope",
        _ => "Unverified",
    })
}
fn draw(ui: &mut egui::Ui, value: &C2paValue, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    *remaining -= 1;
    match value {
        C2paValue::Map(pairs) => {
            for (index, (key, value)) in pairs.iter().enumerate() {
                if *remaining == 0 {
                    break;
                }
                ui.push_id(index, |ui| {
                    ui.collapsing(brief(key), |ui| draw(ui, value, remaining));
                });
            }
        }
        C2paValue::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                if *remaining == 0 {
                    break;
                }
                ui.push_id(index, |ui| {
                    ui.collapsing(index.to_string(), |ui| draw(ui, value, remaining));
                });
            }
        }
        C2paValue::Tag { number, value } => {
            ui.label(format!("CBOR tag {number}"));
            draw(ui, value, remaining);
        }
        _ => {
            ui.monospace(brief(value));
        }
    }
}
impl Desktop {
    pub(crate) fn c2pa_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(crate::i18n::tr("Content Credentials (C2PA)"));
        ui.label(crate::i18n::tr("Cryptographic checks do not establish signer trust or content truth. Full C2PA validity has not been established; declarations must not be attributed to a signer."));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(crate::i18n::tr(
                    "Review Content Credentials in current scope",
                )),
            )
            .clicked()
        {
            match (self.case.clone(), self.active_query(self.analysis.selected)) {
                (Some(case), Ok(query)) => self.task(
                    ui.ctx(),
                    "Reading Content Credentials",
                    move || {
                        let revision = case.info()?.revision;
                        let total = case.count_search(&query)?;
                        let page = case.search_records(&query, None, 200)?;
                        let mut records = Vec::new();
                        let mut bytes = 0;
                        for hit in page.items {
                            let record = case
                                .record(hit.id)?
                                .ok_or_else(|| std::io::Error::other("Record disappeared"))?;
                            bytes += serde_json::to_vec(&record)
                                .map_err(std::io::Error::other)?
                                .len();
                            if bytes > AnalysisLimits::default().max_bytes {
                                return Err(std::io::Error::other(
                                    "Narrow the file filter to fit the Content Credentials budget",
                                ));
                            }
                            records.push(record);
                        }
                        let reports = app::analysis::c2pa::extract(
                            &records,
                            AnalysisLimits {
                                max_records: 200,
                                max_results: 1000,
                                ..Default::default()
                            },
                        )?;
                        if case.info()?.revision != revision {
                            return Err(std::io::Error::other(
                                "Case changed; review Content Credentials again",
                            ));
                        }
                        Ok(C2paUi {
                            reports,
                            scan: query.records.scan_id,
                            revision,
                            total,
                            loaded: true,
                        })
                    },
                    |state, result| state.analysis.c2pa = result,
                ),
                (_, Err(error)) => self.report_error(error),
                _ => {}
            }
        }
        let state = &self.analysis.c2pa;
        if !state.loaded {
            return;
        }
        if self.active_scan.as_ref() != Some(&state.scan)
            || self
                .info
                .as_ref()
                .is_some_and(|i| i.revision != state.revision)
        {
            ui.colored_label(
                egui::Color32::YELLOW,
                crate::i18n::tr(
                    "Older Content Credentials snapshot; review again for the current scope.",
                ),
            );
        }
        if state.total > 200 {
            ui.label(crate::i18n::tr(
                "This review covers the first 200 files. Narrow the filter for other files.",
            ));
        }
        if state.reports.is_empty() {
            ui.label(crate::i18n::tr(
                "No embedded Content Credentials in this selection.",
            ));
        }
        let mut open = None;
        let mut remaining = 1000;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for report in &state.reports {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.strong(report.source.to_string());
                    for verification in &report.verifications {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(&verification.manifest_label);
                            ui.label(crate::textf!(
                                "Claim signature: {}",
                                "주장 서명: {}",
                                verdict(&verification.signature_integrity)
                            ));
                            ui.label(crate::textf!(
                                "Assertion hashes: {}",
                                "주장 해시: {}",
                                verdict(&verification.assertion_integrity)
                            ));
                            ui.label(crate::textf!(
                                "File binding: {}",
                                "파일 바인딩: {}",
                                verdict(&verification.content_binding)
                            ));
                            ui.label(crate::i18n::tr("Signer trust is unknown"));
                            if let Some(fingerprint) = &verification.certificate_sha256 {
                                ui.monospace(format!("SHA-256: {fingerprint}"));
                            }
                            ui.collapsing(
                                crate::i18n::tr("Verification scope and limitations"),
                                |ui| {
                                    ui.label(&verification.scope);
                                    ui.label(format!(
                                        "{} · {} · {}",
                                        verification.signature_integrity,
                                        verification.assertion_integrity,
                                        verification.content_binding
                                    ));
                                    ui.weak(crate::textf!(
                                        "{} supporting observations",
                                        "근거 관찰값 {}개",
                                        verification.supporting_observations.len()
                                    ));
                                },
                            );
                        });
                    }
                    for issue in &report.issues {
                        ui.colored_label(egui::Color32::YELLOW, issue);
                    }
                    for object in &report.boxes {
                        if let Some(value) = &object.decoded {
                            let label = object
                                .parent
                                .as_ref()
                                .and_then(|parent| {
                                    report.boxes.iter().find(|b| &b.identity == parent)
                                })
                                .and_then(|b| b.label.as_deref())
                                .unwrap_or(&object.box_type);
                            ui.push_id(&object.identity, |ui| {
                                ui.collapsing(label, |ui| {
                                    draw(ui, value, &mut remaining);
                                    ui.weak(crate::textf!(
                                        "Raw packet observation {}",
                                        "원시 패킷 관찰값 {}",
                                        object.payload_observation.map_or(0, |id| id.get())
                                    ));
                                });
                            });
                        }
                        if let Some(issue) = &object.issue {
                            ui.label(format!("{}: {issue}", object.identity));
                        }
                    }
                    ui.collapsing(crate::i18n::tr("Original JUMBF inventory"), |ui| {
                        for object in &report.boxes {
                            ui.label(format!(
                                "{} · {} · {} bytes",
                                object.label.as_deref().unwrap_or(&object.box_type),
                                object.uuid.as_deref().unwrap_or(""),
                                object.size
                            ));
                        }
                    });
                    if ui.button(crate::i18n::tr("Open selected file")).clicked() {
                        open = Some((state.scan.clone(), report.source.clone()));
                    }
                });
            }
        });
        if remaining == 0 {
            ui.label(crate::i18n::tr(
                "View truncated; complete raw packets remain in the case.",
            ));
        }
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
