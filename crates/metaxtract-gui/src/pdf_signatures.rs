use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{
        AnalysisLimits,
        pdf_signatures::{CertificatePathStatus, PdfSignatureAssessment},
    },
};

#[derive(Default)]
pub struct SignaturesUi {
    reports: Vec<PdfSignatureAssessment>,
    scan: String,
    revision: u64,
    total: u64,
    loaded: bool,
}
fn integrity_label(status: &str) -> &'static str {
    crate::i18n::tr(match status {
        "valid_for_signed_revision" => "Signature valid for the signed revision",
        "invalid" => "Signature integrity failed",
        "unsupported_subfilter" => "Unsupported signature type",
        "resource_limit" => "Signature verification reached a resource limit",
        _ => "Signature integrity is unverified",
    })
}
impl Desktop {
    pub(crate) fn signatures_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(crate::i18n::tr("PDF signatures and certificates"));
        ui.label(crate::i18n::tr("Certificate paths use only your configured anchors. Revocation, trusted signing time and document truth remain unverified."));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(crate::i18n::tr(
                    "Review signatures in current filter / selection",
                )),
            )
            .clicked()
        {
            match (self.case.clone(), self.active_query(self.analysis.selected)) {
                (Some(case), Ok(query)) => self.task(
                    ui.ctx(),
                    "Reviewing PDF signatures",
                    move || {
                        let revision = case.info()?.revision;
                        let total = case.count_search(&query)?;
                        let page = case.search_records(&query, None, 200)?;
                        let mut records = Vec::new();
                        let mut bytes = 0;
                        for hit in page.items {
                            let record = case
                                .record(hit.id)?
                                .ok_or_else(|| std::io::Error::other("record disappeared"))?;
                            bytes += serde_json::to_vec(&record)
                                .map_err(std::io::Error::other)?
                                .len();
                            if bytes > AnalysisLimits::default().max_bytes {
                                return Err(std::io::Error::other(
                                    "Narrow the file filter to fit the signature review budget",
                                ));
                            }
                            records.push(record);
                        }
                        let reports = app::analysis::pdf_signatures::assess(
                            &records,
                            AnalysisLimits {
                                max_records: 200,
                                max_results: 1000,
                                ..Default::default()
                            },
                        )?;
                        if case.info()?.revision != revision {
                            return Err(std::io::Error::other(
                                "Case changed; review signatures again",
                            ));
                        }
                        Ok(SignaturesUi {
                            reports,
                            scan: query.records.scan_id,
                            revision,
                            total,
                            loaded: true,
                        })
                    },
                    |state, result| state.analysis.signatures = result,
                ),
                (_, Err(error)) => self.report_error(error),
                _ => {}
            }
        }
        let state = &self.analysis.signatures;
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
                crate::i18n::tr("Older signature review; rebuild for the current scope."),
            );
        }
        if state.total > 200 {
            ui.label(crate::i18n::tr(
                "Signature review covers the first 200 files. Narrow the filter for other files.",
            ));
        }
        if state.reports.is_empty() {
            ui.label(crate::i18n::tr(
                "No PDF signature evidence in this selection.",
            ));
        }
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for report in &state.reports {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.strong(format!("{} · {}", report.source, report.signature));
                    ui.label(integrity_label(&report.integrity));
                    ui.label(crate::i18n::tr(match report.certificate_path {
                        CertificatePathStatus::ValidToConfiguredAnchor => {
                            "Certificate path valid to a configured anchor"
                        }
                        CertificatePathStatus::Untrusted => {
                            "Certificate path or signing usage failed"
                        }
                        CertificatePathStatus::Unknown => "Certificate trust is unknown",
                    }));
                    ui.label(crate::i18n::tr(if report.covers_current_file {
                        "The signature covers the current file bytes"
                    } else {
                        "Current file coverage is incomplete or unverified"
                    }));
                    if let Some(tail) = report.unsigned_tail_bytes {
                        ui.label(crate::textf!(
                            "Unsigned tail: {} bytes",
                            "미서명 꼬리: {}바이트",
                            tail
                        ));
                    }
                    if let Some(time) = report.evaluation_unix_seconds {
                        ui.label(crate::textf!(
                            "Certificate evaluation time (Unix UTC): {}",
                            "인증서 평가 시각 (Unix UTC): {}",
                            time
                        ));
                    }
                    ui.collapsing(crate::i18n::tr("Certificate details"), |ui| {
                        for (key, value) in &report.certificates {
                            let (owner, field) = key.split_once('.').unwrap_or((key, ""));
                            let label = crate::i18n::tr(match field {
                                "subject" => "Subject",
                                "issuer" => "Issuer",
                                "sha256" => "SHA-256 fingerprint",
                                "not_before" => "Valid from",
                                "not_after" => "Valid until",
                                "serial" => "Serial number",
                                "chain_result" => "Certificate chain result",
                                "chain_error_code" => "Certificate chain error code",
                                "chain_sha256" => "Certificate chain fingerprints",
                                _ => field,
                            });
                            ui.label(format!("{owner} · {label}: {value}"));
                        }
                        ui.weak(crate::textf!(
                            "{} supporting observations",
                            "근거 관찰값 {}개",
                            report.supporting_observations.len()
                        ));
                    });
                    if ui.button(crate::i18n::tr("Open selected file")).clicked() {
                        open = Some((state.scan.clone(), report.source.clone()));
                    }
                });
            }
        });
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
