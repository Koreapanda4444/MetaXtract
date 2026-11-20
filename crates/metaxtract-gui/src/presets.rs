use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app::storage::{SearchExpression, SearchField, SearchQuery};
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    query: Option<SearchQuery>,
    case_id: Option<String>,
    action: usize,
    format: usize,
    scope: usize,
    profile: String,
    redact: bool,
    radius: f64,
}
fn contains_record_ids(expression: &SearchExpression) -> bool {
    match expression {
        SearchExpression::Predicate(p) => matches!(p.field, SearchField::RecordId),
        SearchExpression::All(v) | SearchExpression::Any(v) => v.iter().any(contains_record_ids),
        SearchExpression::Not(v) => contains_record_ids(v),
    }
}
impl Desktop {
    pub(crate) fn preset_controls(&mut self, ui: &mut egui::Ui) {
        let mut apply = None;
        let mut remove = None;
        let mut save = false;
        ui.collapsing(crate::i18n::tr("Saved presets"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.workflows.preset_name)
                        .hint_text(crate::i18n::tr("Preset name"))
                        .char_limit(128),
                );
                if ui
                    .add_enabled(
                        !self.workflows.preset_name.trim().is_empty(),
                        egui::Button::new(crate::i18n::tr("Save applied filter and settings")),
                    )
                    .clicked()
                {
                    save = true;
                }
            });
            for (i, preset) in self.settings.presets.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(&preset.name);
                    if ui
                        .add_enabled(!self.busy, egui::Button::new(crate::i18n::tr("Apply")))
                        .clicked()
                    {
                        apply = Some(preset.clone());
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if save {
            let result = (|| -> std::io::Result<_> {
                let name = self.workflows.preset_name.trim().to_owned();
                if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
                    return Err(std::io::Error::other("Invalid preset name"));
                }
                if self.workflows.profile.len() > 65536 {
                    return Err(std::io::Error::other("Profile exceeds 64 KiB"));
                }
                let profile: metaxtract_app::analysis::redaction::RedactionProfile =
                    serde_json::from_str(&self.workflows.profile).map_err(std::io::Error::other)?;
                profile.validate()?;
                Ok(Preset {
                    name,
                    query: self.active_query(false).ok(),
                    case_id: self.info.as_ref().map(|i| i.case_id.clone()),
                    action: if matches!(self.workflows.action, 0..=2) {
                        self.workflows.action
                    } else {
                        0
                    },
                    format: self.workflows.format.min(1),
                    scope: self.workflows.scope.min(2),
                    profile: self.workflows.profile.clone(),
                    redact: self.workflows.redact,
                    radius: self.analysis.radius,
                })
            })();
            match result {
                Ok(p) => {
                    self.settings.presets.retain(|old| old.name != p.name);
                    self.settings.presets.insert(0, p);
                    self.settings.presets.truncate(32);
                    if let Err(e) = self.settings.save() {
                        self.report_error(e);
                    }
                }
                Err(e) => self.report_error(e),
            }
        }
        if let Some(i) = remove {
            self.settings.presets.remove(i);
            if let Err(e) = self.settings.save() {
                self.report_error(e);
            }
        }
        if let Some(preset) = apply {
            if let Err(e) = self.apply_preset(preset) {
                self.report_error(e);
            }
        }
    }
    fn apply_preset(&mut self, p: Preset) -> std::io::Result<()> {
        if p.action > 2
            || p.format > 1
            || p.scope > 2
            || !p.radius.is_finite()
            || !(0.0..=1_000_000.0).contains(&p.radius)
            || p.profile.len() > 65536
        {
            return Err(std::io::Error::other("Invalid preset settings"));
        }
        let profile: metaxtract_app::analysis::redaction::RedactionProfile =
            serde_json::from_str(&p.profile).map_err(std::io::Error::other)?;
        profile.validate()?;
        if let Some(mut q) = p.query {
            let scan = self.active_scan.clone().ok_or_else(|| {
                std::io::Error::other("Select a scan before applying a saved filter")
            })?;
            if contains_record_ids(&q.expression)
                && (p.case_id.as_deref() != self.info.as_ref().map(|i| i.case_id.as_str())
                    || q.records.scan_id != scan)
            {
                return Err(std::io::Error::other(
                    "This preset contains record IDs from another scan. Select its original case and scan.",
                ));
            }
            q.records.scan_id = scan;
            q.validate()?;
            let tab = self.tab;
            let selected = self.records.selected.clone();
            self.search.visual.load(q.clone());
            self.search.text.clear();
            self.search.advanced = false;
            self.apply_search(q);
            self.records.selected = selected;
            self.tab = tab;
        }
        self.workflows.select_action(p.action);
        self.workflows.format = p.format;
        self.workflows.scope = p.scope;
        self.workflows.selected = p.scope == 2;
        self.workflows.profile = p.profile;
        self.workflows.redact = p.redact;
        self.analysis.radius = p.radius;
        Ok(())
    }
}
