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
    advanced_tab: Option<InspectorTab>,
    open_evidence: bool,
    record: Option<i64>,
    filter: String,
    cached_filter: String,
    raw_indices: Vec<usize>,
    claim_indices: Vec<usize>,
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
    limit: usize,
    bytes: Vec<u8>,
}
impl Write for Bounded {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let count = data.len().min(self.limit.saturating_sub(self.bytes.len()));
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
    let mut output = Bounded {
        limit: 16384,
        bytes: vec![],
    };
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
            self.inspect.advanced_tab = Some(InspectorTab::Raw);
            self.inspect.raw_indices = (0..record.observations.len()).collect();
            self.inspect.groups = crate::categories::groups(record);
            self.inspect.claim_indices = (0..record.claims.len()).collect();
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.inspect.filter)
                .hint_text(crate::i18n::tr("Search metadata names and values"))
                .char_limit(256)
                .desired_width(f32::INFINITY),
        );
        if self.inspect.filter != self.inspect.cached_filter {
            let filter = self.inspect.filter.to_lowercase();
            self.inspect.raw_indices = record
                .observations
                .iter()
                .enumerate()
                .filter(|(_, o)| {
                    let name = format!(
                        "{} {} {:?} {}",
                        o.tag.namespace,
                        o.tag.name.as_deref().unwrap_or(""),
                        o.tag.code,
                        o.unit.as_deref().unwrap_or("")
                    );
                    crate::metadata_search::contains(&name, &filter)
                        || crate::metadata_search::raw(&o.raw_value, &filter)
                        || o.interpreted_value
                            .as_ref()
                            .is_some_and(|v| crate::metadata_search::raw(v, &filter))
                })
                .map(|(i, _)| i)
                .collect();
            self.inspect.claim_indices = record
                .claims
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    crate::metadata_search::contains(c.key.as_str(), &filter)
                        || crate::metadata_search::normalized(&c.value, &filter)
                })
                .map(|(i, _)| i)
                .collect();
            self.inspect.groups = crate::categories::groups(record);
            for (_, indices) in &mut self.inspect.groups {
                indices.retain(|i| self.inspect.raw_indices.binary_search(i).is_ok());
            }
            self.inspect
                .groups
                .retain(|(_, indices)| !indices.is_empty());
            self.inspect.selected_raw = None;
            self.inspect.selected_claim = None;
            self.inspect.cached_filter = self.inspect.filter.clone();
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.inspect.tab,
                InspectorTab::Summary,
                crate::i18n::tr("Summary"),
            );
            ui.selectable_value(
                &mut self.inspect.tab,
                InspectorTab::Preview,
                crate::i18n::tr("Preview"),
            );
        });
        self.render_inspector_tab(ui, self.inspect.tab);
        ui.separator();
        let force_open = self.inspect.open_evidence.then_some(true);
        self.inspect.open_evidence = false;
        egui::CollapsingHeader::new(crate::i18n::tr("Advanced evidence and diagnostics"))
            .id_salt("advanced-evidence")
            .open(force_open)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, tab) in [
                        ("Raw", InspectorTab::Raw),
                        ("Normalized", InspectorTab::Normalized),
                        ("Provenance", InspectorTab::Provenance),
                        ("Conflicts", InspectorTab::Conflicts),
                        ("Diagnostics", InspectorTab::Diagnostics),
                    ] {
                        ui.selectable_value(
                            &mut self.inspect.advanced_tab,
                            Some(tab),
                            crate::i18n::tr(label),
                        );
                    }
                });
                self.render_inspector_tab(
                    ui,
                    self.inspect.advanced_tab.unwrap_or(InspectorTab::Raw),
                );
            });
    }
    fn render_inspector_tab(&mut self, ui: &mut egui::Ui, tab: InspectorTab) {
        if tab == InspectorTab::Preview {
            self.preview_view(ui);
            return;
        }
        let (_, record) = self.records.focused.as_ref().expect("focused record");
        match tab {
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
                                    let response = ui
                                        .add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::selectable(
                                                self.inspect.selected_raw == Some(index),
                                                text,
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&observation.tag.namespace);
                                    response.clone().context_menu(|ui| {
                                        copy_field(
                                            ui,
                                            &name,
                                            &observation.raw_value,
                                            observation,
                                            match &observation.raw_value {
                                                RawValue::Text(v) => Some(v.text.as_str()),
                                                _ => None,
                                            },
                                            &mut self.notifications,
                                        )
                                    });
                                    if response.clicked() {
                                        self.inspect.selected_raw = Some(index);
                                        self.inspect.advanced_tab = Some(InspectorTab::Raw);
                                        self.inspect.open_evidence = true;
                                    }
                                }
                            });
                    });
                }
            }
            InspectorTab::Preview => {}
            InspectorTab::Raw => {
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
                    ui.menu_button(crate::i18n::tr("Copy"), |ui| {
                        copy_field(
                            ui,
                            o.tag.name.as_deref().unwrap_or(&o.tag.namespace),
                            &o.raw_value,
                            o,
                            match &o.raw_value {
                                RawValue::Text(v) => Some(v.text.as_str()),
                                _ => None,
                            },
                            &mut self.notifications,
                        )
                    });
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
                    .show_rows(ui, 26.0, self.inspect.claim_indices.len(), |ui, range| {
                        for &i in &self.inspect.claim_indices[range] {
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
                                self.inspect.advanced_tab = Some(InspectorTab::Normalized);
                                self.inspect.open_evidence = true;
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

fn copy_json(value: &impl serde::Serialize) -> io::Result<String> {
    let mut output = Bounded {
        bytes: vec![],
        limit: 1024 * 1024,
    };
    serde_json::to_writer_pretty(&mut output, value).map_err(io::Error::other)?;
    String::from_utf8(output.bytes).map_err(io::Error::other)
}
fn copy_field(
    ui: &mut egui::Ui,
    name: &str,
    raw: &impl serde::Serialize,
    field: &impl serde::Serialize,
    text: Option<&str>,
    notices: &mut crate::notifications::Notifications,
) {
    for (label, kind) in [
        ("Copy field name", 0),
        ("Copy value", 1),
        ("Copy field JSON", 2),
    ] {
        if ui.button(crate::i18n::tr(label)).clicked() {
            let result = match kind {
                0 => Ok(name.to_owned()),
                1 => match text {
                    Some(text) if text.len() <= 1024 * 1024 => Ok(text.to_owned()),
                    Some(_) => Err(io::Error::other(
                        "clipboard limit is 1 MiB; export the complete value",
                    )),
                    None => copy_json(raw),
                },
                _ => copy_json(field),
            };
            match result {
                Ok(text) => ui.ctx().copy_text(text),
                Err(error) => {
                    notices.push(crate::i18n::tr("Operation failed"), error.to_string(), true)
                }
            }
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_rejects_oversized_values_without_returning_partial_json() {
        let text = "metadata".repeat(200_000);
        assert!(copy_json(&text).is_err());
        let text = "한글\nmetadata";
        let copied = copy_json(&text).unwrap();
        assert_eq!(serde_json::from_str::<String>(&copied).unwrap(), text);
    }
}
