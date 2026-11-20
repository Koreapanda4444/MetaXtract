use eframe::egui;
use metaxtract_app::analysis::redaction::{
    RedactionAction as A, RedactionProfile, RedactionRule, Selector as S,
};
pub(crate) fn form(ui: &mut egui::Ui, json: &mut String) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::i18n::tr("Privacy preset")).clicked() {
            *json = serde_json::to_string_pretty(&RedactionProfile::privacy())
                .expect("privacy profile");
        }
        if ui.button(crate::i18n::tr("Remove all metadata")).clicked() {
            let mut p = RedactionProfile::privacy();
            p.name = "all-metadata".into();
            p.rules = vec![RedactionRule {
                selector: S::AllMetadata,
                action: A::Remove,
            }];
            *json = serde_json::to_string_pretty(&p).expect("built-in profile");
        }
    });
    let parsed = serde_json::from_str::<RedactionProfile>(json);
    if let Ok(mut profile) = parsed {
        let before = serde_json::to_string(&profile).expect("profile");
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Profile name"));
            ui.add(egui::TextEdit::singleline(&mut profile.name).char_limit(256));
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut profile.strip_encoded_payloads,
                crate::i18n::tr("Remove encoded payloads"),
            );
            ui.checkbox(
                &mut profile.omit_unstructured_context,
                crate::i18n::tr("Remove unstructured context"),
            );
            ui.checkbox(
                &mut profile.hide_paths,
                crate::i18n::tr("Hide evidence paths"),
            );
            ui.checkbox(
                &mut profile.omit_digests,
                crate::i18n::tr("Omit content digests"),
            );
            ui.checkbox(
                &mut profile.omit_filesystem_times,
                crate::i18n::tr("Omit filesystem times"),
            );
        });
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("redaction-rule-form")
            .max_height(260.0)
            .show(ui, |ui| {
                for (i, rule) in profile.rules.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("{}.", i + 1));
                            let mut kind = match rule.selector {
                                S::Privacy => 0,
                                S::AllMetadata => 1,
                                S::ClaimPrefix(_) => 2,
                                S::RawNamespacePrefix(_) => 3,
                                S::RawName(_) => 4,
                            };
                            let previous = kind;
                            egui::ComboBox::from_id_salt("selector")
                                .selected_text(crate::i18n::tr(
                                    [
                                        "Privacy fields",
                                        "All metadata",
                                        "Claim prefix",
                                        "Raw namespace prefix",
                                        "Raw field name",
                                    ][kind],
                                ))
                                .show_ui(ui, |ui| {
                                    for (n, label) in [
                                        "Privacy fields",
                                        "All metadata",
                                        "Claim prefix",
                                        "Raw namespace prefix",
                                        "Raw field name",
                                    ]
                                    .iter()
                                    .enumerate()
                                    {
                                        ui.selectable_value(&mut kind, n, crate::i18n::tr(label));
                                    }
                                });
                            if previous != kind {
                                rule.selector = match kind {
                                    0 => S::Privacy,
                                    1 => S::AllMetadata,
                                    2 => S::ClaimPrefix("metadata.author".into()),
                                    3 => S::RawNamespacePrefix("exif".into()),
                                    _ => S::RawName("Author".into()),
                                };
                            }
                            match &mut rule.selector {
                                S::ClaimPrefix(s) | S::RawNamespacePrefix(s) | S::RawName(s) => {
                                    ui.add(
                                        egui::TextEdit::singleline(s)
                                            .desired_width(190.0)
                                            .char_limit(4096),
                                    );
                                }
                                _ => {}
                            }
                            let mut mask = matches!(rule.action, A::Mask(_));
                            if ui
                                .checkbox(&mut mask, crate::i18n::tr("Mask instead of remove"))
                                .changed()
                            {
                                rule.action = if mask {
                                    A::Mask("[redacted]".into())
                                } else {
                                    A::Remove
                                };
                            }
                            if let A::Mask(value) = &mut rule.action {
                                ui.add(
                                    egui::TextEdit::singleline(value)
                                        .desired_width(150.0)
                                        .char_limit(4096)
                                        .hint_text(crate::i18n::tr("Replacement")),
                                );
                            }
                            if ui.small_button("×").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
            });
        if let Some(i) = remove {
            profile.rules.remove(i);
        }
        if ui
            .add_enabled(
                profile.rules.len() < 128,
                egui::Button::new(crate::i18n::tr("Add rule")),
            )
            .clicked()
        {
            profile.rules.push(RedactionRule {
                selector: S::Privacy,
                action: A::Remove,
            });
        }
        if let Err(e) = profile.validate() {
            ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
        }
        if serde_json::to_string(&profile).expect("profile") != before {
            *json = serde_json::to_string_pretty(&profile).expect("profile");
        }
    } else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            crate::i18n::tr(
                "Profile JSON is invalid. Restore a preset or repair the advanced editor.",
            ),
        );
    }
    ui.weak(crate::i18n::tr(
        "Changes apply to exported metadata; original files and case records are preserved.",
    ));
    ui.collapsing(crate::i18n::tr("Advanced profile JSON"), |ui| {
        ui.add(
            egui::TextEdit::multiline(json)
                .code_editor()
                .desired_rows(6)
                .desired_width(f32::INFINITY)
                .char_limit(65536),
        );
    });
}
