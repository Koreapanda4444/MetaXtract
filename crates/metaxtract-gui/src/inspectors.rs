use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::{self as app, schema::*};
use std::io::{self, Write};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum InspectorTab {
    #[default]
    Summary,
    Preview,
    Raw,
    Normalized,
    Provenance,
    Conflicts,
    Diagnostics,
}
#[derive(Default)]
pub struct InspectUi {
    pub tab: InspectorTab,
    record: Option<i64>,
    filter: String,
    cached_filter: String,
    raw_indices: Vec<usize>,
    groups: Vec<(String, Vec<usize>)>,
    selected_raw: Option<usize>,
    selected_claim: Option<usize>,
    selected_conflict: Option<usize>,
    selected_diagnostic: Option<usize>,
    selected_lineage: Option<usize>,
    pub lineage: Option<Vec<app::analysis::provenance::FieldLineage>>,
}
fn shortened(s: &str, max: usize) -> String {
    let mut out = s.chars().take(max).collect::<String>();
    if s.chars().nth(max).is_some() {
        out.push_str(crate::i18n::tr(" … [view truncated"));
    }
    out
}
pub(crate) fn raw_brief(value: &RawValue) -> String {
    crate::formatting::raw(value, None)
}
pub(crate) fn claim_brief(value: &NormalizedValue) -> String {
    crate::formatting::normalized(value)
}
struct Bounded {
    bytes: Vec<u8>,
}
impl Write for Bounded {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let count = data.len().min(16384usize.saturating_sub(self.bytes.len()));
        if count == 0 {
            return Err(io::Error::other("view limit"));
        }
        self.bytes.extend_from_slice(&data[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn json_view(ui: &mut egui::Ui, value: &impl serde::Serialize) {
    let mut output = Bounded { bytes: vec![] };
    let truncated = serde_json::to_writer_pretty(&mut output, value).is_err();
    let mut text = String::from_utf8_lossy(&output.bytes).into_owned();
    if truncated {
        text.push_str(crate::i18n::tr(
            "\n[View truncated; complete data remains in exports",
        ));
    }
    ui.add(
        egui::Label::new(egui::RichText::new(text).monospace())
            .selectable(true)
            .wrap(),
    );
}
impl Desktop {
    pub fn inspector_view(&mut self, ui: &mut egui::Ui) {
        let Some((id, record)) = self.records.focused.as_ref() else {
            return;
        };
        if self.inspect.record != Some(*id) {
            self.inspect = InspectUi::default();
            self.inspect.record = Some(*id);
            self.inspect.raw_indices = (0..record.observations.len()).collect();
            self.inspect.groups = crate::categories::groups(record);
        }
        ui.horizontal_wrapped(|ui| {
            for (label, tab) in [
                (crate::i18n::tr("Summary"), InspectorTab::Summary),
                (crate::i18n::tr("Preview"), InspectorTab::Preview),
                (crate::i18n::tr("Raw"), InspectorTab::Raw),
                (crate::i18n::tr("Normalized"), InspectorTab::Normalized),
                (crate::i18n::tr("Provenance"), InspectorTab::Provenance),
                (crate::i18n::tr("Conflicts"), InspectorTab::Conflicts),
                (crate::i18n::tr("Diagnostics"), InspectorTab::Diagnostics),
            ] {
                ui.selectable_value(&mut self.inspect.tab, tab, label);
            }
        });
        if self.inspect.tab == InspectorTab::Preview {
            self.preview_view(ui);
            return;
        }
        let (_, record) = self.records.focused.as_ref().expect("focused record");
        match self.inspect.tab {
            InspectorTab::Summary => {
                crate::summary::overview(ui, record);
                ui.separator();
                for (category, indices) in &self.inspect.groups {
                    egui::CollapsingHeader::new(format!(
                        "{} · {}",
                        crate::i18n::tr(category),
                        indices.len()
                    ))
                    .id_salt(("semantic-group", category))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(category)
                            .max_height(240.0)
                            .show_rows(ui, 26.0, indices.len(), |ui, range| {
                                for &index in &indices[range] {
                                    let observation = &record.observations[index];
                                    let name =
                                        observation.tag.name.clone().unwrap_or_else(|| {
                                            format!("{:?}", observation.tag.code)
                                        });
                                    let original = crate::formatting::raw(
                                        &observation.raw_value,
                                        observation.unit.as_deref(),
                                    );
                                    let interpreted = observation
                                        .interpreted_value
                                        .as_ref()
                                        .filter(|v| **v != observation.raw_value)
                                        .map(|v| {
                                            format!(
                                                " · {}: {}",
                                                crate::i18n::tr("Interpreted"),
                                                crate::formatting::raw(
                                                    v,
                                                    observation.unit.as_deref()
                                                )
                                            )
                                        })
                                        .unwrap_or_default();
                                    let text = format!("{name} = {original}{interpreted}");
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::selectable(
                                                self.inspect.selected_raw == Some(index),
                                                text,
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&observation.tag.namespace)
                                        .clicked()
                                    {
                                        self.inspect.selected_raw = Some(index);
                                        self.inspect.tab = InspectorTab::Raw;
                                    }
                                }
                            });
                    });
                }
            }
            InspectorTab::Preview => {}
            InspectorTab::Raw => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Find namespace or tag"));
                    ui.text_edit_singleline(&mut self.inspect.filter);
                });
                if self.inspect.filter != self.inspect.cached_filter {
                    let filter = self.inspect.filter.to_lowercase();
                    self.inspect.raw_indices = record
                        .observations
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| {
                            format!(
                                "{} {} {:?}",
                                o.tag.namespace,
                                o.tag.name.as_deref().unwrap_or(""),
                                o.tag.code
                            )
                            .to_lowercase()
                            .contains(&filter)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    self.inspect.cached_filter = self.inspect.filter.clone();
                }
                ui.label(format!(
                    "{} / {} observations",
                    self.inspect.raw_indices.len(),
                    record.observations.len()
                ));
                egui::ScrollArea::vertical()
                    .id_salt("raw-fields")
                    .max_height(240.0)
                    .show_rows(ui, 26.0, self.inspect.raw_indices.len(), |ui, range| {
                        for &i in &self.inspect.raw_indices[range] {
                            let o = &record.observations[i];
                            if ui
                                .selectable_label(
                                    self.inspect.selected_raw == Some(i),
                                    format!(
                                        "#{} · {} · {} = {}",
                                        o.id.get(),
                                        shortened(&o.tag.namespace, 120),
                                        shortened(o.tag.name.as_deref().unwrap_or("unknown"), 160),
                                        raw_brief(&o.raw_value)
                                    ),
                                )
                                .clicked()
                            {
                                self.inspect.selected_raw = Some(i);
                            }
                        }
                    });
                if let Some(o) = self
                    .inspect
                    .selected_raw
                    .and_then(|i| record.observations.get(i))
                {
                    ui.separator();
                    ui.strong(crate::i18n::tr("Original value and decoding"));
                    match &o.raw_value {
                        RawValue::Bytes(bytes) => {
                            ui.monospace(format!(
                                "{} bytes; first 128 bytes: {}",
                                bytes.len(),
                                bytes
                                    .iter()
                                    .take(128)
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>()
                            ));
                        }
                        v => json_view(ui, v),
                    }
                    if let Some(value) = &o.interpreted_value {
                        ui.label(crate::i18n::tr("Interpreted alongside original"));
                        json_view(ui, value);
                    }
                    ui.label(crate::i18n::tr("Provenance"));
                    json_view(ui, &o.provenance);
                }
            }
            InspectorTab::Normalized => {
                ui.label(crate::i18n::tr(
                    "Derived claims retain confidence, alternatives and raw observation links.",
                ));
                egui::ScrollArea::vertical()
                    .id_salt("normalized-fields")
                    .max_height(240.0)
                    .show_rows(ui, 26.0, record.claims.len(), |ui, range| {
                        for i in range {
                            let c = &record.claims[i];
                            if ui
                                .selectable_label(
                                    self.inspect.selected_claim == Some(i),
                                    format!(
                                        "#{} · {} = {} · {}‰ · {:?}",
                                        c.id.get(),
                                        c.key,
                                        claim_brief(&c.value),
                                        c.confidence.score_millis(),
                                        c.resolution
                                    ),
                                )
                                .clicked()
                            {
                                self.inspect.selected_claim = Some(i);
                            }
                        }
                    });
                if let Some(claim) = self
                    .inspect
                    .selected_claim
                    .and_then(|i| record.claims.get(i))
                {
                    json_view(ui, claim);
                }
            }
            InspectorTab::Provenance => {
                ui.label(crate::i18n::tr("Source identity"));
                json_view(ui, &record.source);
                json_view(ui, &record.digests);
                if ui
                    .add_enabled(
                        !self.busy,
                        crate::components::button(crate::i18n::tr("Resolve field lineage")),
                    )
                    .clicked()
                {
                    let record = record.clone();
                    let id = self.records.focused.as_ref().expect("focused record").0;
                    self.task(
                        ui.ctx(),
                        crate::i18n::tr("Resolving provenance"),
                        move || {
                            app::analysis::provenance::field_lineage(&record, Default::default())
                        },
                        move |app, lineage| {
                            if app.records.focused.as_ref().map(|r| r.0) == Some(id) {
                                app.inspect.lineage = Some(lineage);
                            }
                        },
                    );
                }
                if let Some(lineage) = &self.inspect.lineage {
                    egui::ScrollArea::vertical()
                        .id_salt("field-lineage")
                        .max_height(240.0)
                        .show_rows(ui, 26.0, lineage.len(), |ui, range| {
                            for i in range {
                                let field = &lineage[i];
                                if ui
                                    .selectable_label(
                                        self.inspect.selected_lineage == Some(i),
                                        format!(
                                            "#{} {} · {} raw links",
                                            field.claim.get(),
                                            field.key,
                                            field.evidence.len()
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.inspect.selected_lineage = Some(i);
                                }
                            }
                        });
                    if let Some(field) = self.inspect.selected_lineage.and_then(|i| lineage.get(i))
                    {
                        json_view(ui, field);
                    }
                }
            }
            InspectorTab::Conflicts => {
                if record.conflicts.is_empty() {
                    ui.label(crate::i18n::tr("No recorded claim conflicts."));
                }
                egui::ScrollArea::vertical()
                    .id_salt("claim-conflicts")
                    .max_height(240.0)
                    .show_rows(ui, 26.0, record.conflicts.len(), |ui, range| {
                        for i in range {
                            let conflict = &record.conflicts[i];
                            if ui
                                .selectable_label(
                                    self.inspect.selected_conflict == Some(i),
                                    format!(
                                        "{} · {}",
                                        conflict.key,
                                        shortened(&conflict.reason, 160)
                                    ),
                                )
                                .clicked()
                            {
                                self.inspect.selected_conflict = Some(i);
                            }
                        }
                    });
                if let Some(conflict) = self
                    .inspect
                    .selected_conflict
                    .and_then(|i| record.conflicts.get(i))
                {
                    json_view(ui, conflict);
                    ui.horizontal_wrapped(|ui| {
                        for id in conflict.alternatives.iter().take(64) {
                            if ui
                                .small_button(crate::textf!("Claim #{}", "해석 #{}", id.get()))
                                .clicked()
                            {
                                self.inspect.selected_claim =
                                    record.claims.iter().position(|c| c.id == *id);
                                self.inspect.tab = InspectorTab::Normalized;
                            }
                        }
                    });
                }
            }
            InspectorTab::Diagnostics => {
                ui.label(crate::textf!(
                    "Record status: {:?}",
                    "레코드 상태: {:?}",
                    record.status
                ));
                if record.diagnostics.is_empty() {
                    ui.label(crate::i18n::tr("No retained diagnostics."));
                }
                egui::ScrollArea::vertical()
                    .id_salt("record-diagnostics")
                    .max_height(240.0)
                    .show_rows(ui, 26.0, record.diagnostics.len(), |ui, range| {
                        for i in range {
                            let d = &record.diagnostics[i];
                            if ui
                                .selectable_label(
                                    self.inspect.selected_diagnostic == Some(i),
                                    format!("{:?} · {:?} · {}", d.severity, d.stage, d.code),
                                )
                                .clicked()
                            {
                                self.inspect.selected_diagnostic = Some(i);
                            }
                        }
                    });
                if let Some(d) = self
                    .inspect
                    .selected_diagnostic
                    .and_then(|i| record.diagnostics.get(i))
                {
                    json_view(ui, d);
                }
            }
        }
    }
}
