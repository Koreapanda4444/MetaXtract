use eframe::egui;
use metaxtract_app::{
    schema::CanonicalKey,
    storage::{
        RecordQuery, SearchExpression as E, SearchField as F, SearchOperator as O, SearchPredicate,
        SearchQuery,
    },
};

pub struct VisualQuery {
    pub expression: E,
    pub base: Option<RecordQuery>,
    epoch: u64,
}
impl Default for VisualQuery {
    fn default() -> Self {
        Self {
            expression: E::All(vec![]),
            base: None,
            epoch: next_epoch(),
        }
    }
}
impl VisualQuery {
    pub fn query(
        &self,
        scan: &str,
        text: &str,
        order: metaxtract_app::storage::RecordOrder,
    ) -> std::io::Result<SearchQuery> {
        let expression = if text.trim().is_empty() {
            self.expression.clone()
        } else {
            E::All(vec![
                self.expression.clone(),
                metaxtract_app::analysis::search::parse(scan, text)?.expression,
            ])
        };
        let mut q = SearchQuery::new(scan, expression);
        if let Some(base) = &self.base {
            q.records = base.clone();
        }
        q.records.scan_id = scan.into();
        q.records.order = order;
        q.validate()?;
        Ok(q)
    }
    pub fn load(&mut self, query: SearchQuery) {
        self.epoch = next_epoch();
        self.expression = query.expression;
        self.base = Some(query.records);
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if let Some(base) = &self.base {
            ui.collapsing(crate::i18n::tr("Saved base restrictions"), |ui| {
                crate::inspectors::json_view(ui, base);
            });
            if ui
                .small_button(crate::i18n::tr("Clear saved base restrictions"))
                .clicked()
            {
                self.base = None;
                changed = true;
            }
        }
        let mut budget = 96usize.saturating_sub(count(&self.expression));
        ui.push_id(("visual-query", self.epoch), |ui| {
            changed |= node(ui, &mut self.expression, 0, &mut budget);
        });
        changed
    }
}
fn count(e: &E) -> usize {
    1 + match e {
        E::All(v) | E::Any(v) => v.iter().map(count).sum(),
        E::Not(v) => count(v),
        _ => 0,
    }
}
fn rule() -> E {
    E::Predicate(SearchPredicate {
        field: F::Mime,
        operator: O::Contains,
        value: String::new(),
    })
}
fn node(ui: &mut egui::Ui, e: &mut E, depth: usize, budget: &mut usize) -> bool {
    let mut changed = false;
    if let E::Predicate(p) = e {
        ui.horizontal_wrapped(|ui| {
            let fields = [
                F::Path,
                F::Mime,
                F::Status,
                F::Size,
                F::Sha256,
                F::Claim(CanonicalKey::parse("image.width").expect("built-in key")),
                F::Tag,
                F::Namespace,
                F::Note,
                F::Conflicts,
                F::Text,
                F::RecordId,
            ];
            egui::ComboBox::from_id_salt("field")
                .selected_text(field_name(&p.field))
                .show_ui(ui, |ui| {
                    for f in fields {
                        if ui
                            .selectable_label(
                                field_name(&p.field) == field_name(&f),
                                field_name(&f),
                            )
                            .clicked()
                        {
                            p.field = f;
                            p.operator = O::Equal;
                            p.value.clear();
                            changed = true;
                        }
                    }
                });
            if let F::Claim(key) = &mut p.field {
                let id = ui.id().with("claim-key-text");
                let mut text = ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<String>(id))
                    .unwrap_or_else(|| key.as_str().to_owned());
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut text)
                            .desired_width(140.0)
                            .char_limit(256),
                    )
                    .changed()
                {
                    ui.ctx().data_mut(|d| d.insert_temp(id, text.clone()));
                }
                let parsed = CanonicalKey::parse(text.clone());
                if ui
                    .add_enabled(
                        parsed.is_ok(),
                        egui::Button::new(crate::i18n::tr("Set key")),
                    )
                    .clicked()
                {
                    *key = parsed.clone().expect("validated key");
                    changed = true;
                }
                if text != key.as_str() {
                    ui.weak(format!("Active key: {key}. Set key to apply this draft."));
                }
                if parsed.is_err() {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        "Use lowercase segments separated by dots.",
                    );
                }
            }
            let ops: &[O] = match p.field {
                F::Size | F::RecordId => &[
                    O::Equal,
                    O::NotEqual,
                    O::Greater,
                    O::GreaterEqual,
                    O::Less,
                    O::LessEqual,
                ],
                F::Conflicts => &[O::Equal, O::NotEqual],
                F::Text => &[O::Contains, O::Equal],
                F::Claim(_) => &[
                    O::Equal,
                    O::NotEqual,
                    O::Contains,
                    O::Greater,
                    O::GreaterEqual,
                    O::Less,
                    O::LessEqual,
                    O::Exists,
                ],
                _ => &[O::Equal, O::NotEqual, O::Contains, O::Exists],
            };
            egui::ComboBox::from_id_salt("operator")
                .selected_text(format!("{:?}", p.operator))
                .show_ui(ui, |ui| {
                    for o in ops {
                        changed |= ui
                            .selectable_value(&mut p.operator, *o, format!("{o:?}"))
                            .changed();
                    }
                });
            if p.operator != O::Exists {
                if matches!(p.field, F::Status | F::Conflicts) {
                    let values: &[&str] = if matches!(p.field, F::Status) {
                        &["Complete", "Partial", "Failed", "Unsupported"]
                    } else {
                        &["true", "false"]
                    };
                    egui::ComboBox::from_id_salt("value")
                        .selected_text(&p.value)
                        .show_ui(ui, |ui| {
                            for value in values {
                                changed |= ui
                                    .selectable_value(&mut p.value, value.to_string(), *value)
                                    .changed();
                            }
                        });
                } else {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut p.value)
                                .desired_width(160.0)
                                .char_limit(4096)
                                .hint_text(crate::i18n::tr("Value")),
                        )
                        .changed();
                }
            }
        });
        return changed;
    }
    let mut kind = match e {
        E::All(_) => 0,
        E::Any(_) => 1,
        _ => 2,
    };
    let previous = kind;
    ui.horizontal(|ui| {
        for (i, label) in ["AND", "OR", "NOT"].iter().enumerate() {
            changed |= ui
                .selectable_value(&mut kind, i, crate::i18n::tr(label))
                .changed();
        }
        if kind == 2 {
            ui.label("negates the group below");
        }
    });
    if kind != previous {
        let old = std::mem::replace(e, E::All(vec![]));
        let children = match old {
            E::All(v) | E::Any(v) => v,
            E::Not(v) => vec![*v],
            _ => unreachable!(),
        };
        *e = match kind {
            0 => E::All(children),
            1 => E::Any(children),
            _ => E::Not(Box::new(E::All(children))),
        };
    }
    ui.indent("children", |ui| match e {
        E::All(children) | E::Any(children) => {
            let mut remove = None;
            for (i, child) in children.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                        ui.vertical(|ui| {
                            changed |= node(ui, child, depth + 1, budget);
                        });
                    });
                });
            }
            if let Some(i) = remove {
                children.remove(i);
                changed = true;
                *budget += 1;
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        *budget > 0 && children.len() < 64,
                        egui::Button::new(crate::i18n::tr("+ condition")),
                    )
                    .clicked()
                {
                    children.push(rule());
                    *budget -= 1;
                    changed = true;
                }
                if ui
                    .add_enabled(
                        *budget > 0 && depth < 5 && children.len() < 64,
                        egui::Button::new(crate::i18n::tr("+ group")),
                    )
                    .clicked()
                {
                    children.push(E::All(vec![]));
                    *budget -= 1;
                    changed = true;
                }
            });
            if children.is_empty() {
                ui.weak(if kind == 0 {
                    "All records (empty AND)"
                } else {
                    "No records (empty OR)"
                });
            }
        }
        E::Not(child) => {
            changed |= node(ui, child, depth + 1, budget);
        }
        _ => {}
    });
    changed
}
fn field_name(f: &F) -> &'static str {
    match f {
        F::Path => "Path",
        F::Mime => "MIME",
        F::Status => "Extraction status",
        F::Size => "Size (bytes)",
        F::Sha256 => "SHA-256",
        F::Claim(_) => "Metadata claim",
        F::Note => "Note",
        F::Tag => "Tag",
        F::Namespace => "Namespace",
        F::Conflicts => "Has conflicts",
        F::Text => "Any text",
        F::RecordId => "Record ID",
    }
}

fn next_epoch() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use metaxtract_app::{schema::*, storage::*};
    #[test]
    fn loaded_boolean_filters_preserve_base_restrictions_and_rebind_scan() {
        let temp = tempfile::tempdir().unwrap();
        let case = CaseWorkspace::create(temp.path().join("q.mxcase"), "query").unwrap();
        let scan = case.begin_scan("fixture", "test").unwrap();
        for (path, mime) in [
            ("photos/a.jpg", "image/jpeg"),
            ("photos/b.png", "image/png"),
            ("other.jpg", "image/jpeg"),
        ] {
            case.append_record(
                &scan,
                &ExtractionRecord::new(SourceDescriptor {
                    path: EvidencePath::root(RelativePath::parse(path).unwrap()),
                    kind: SourceKind::File,
                    identity: FileIdentity {
                        size_bytes: 10,
                        modified_ns: None,
                        created_ns: None,
                        stable_id: None,
                    },
                    declared_media_type: None,
                    detected_media_type: Some(mime.into()),
                }),
            )
            .unwrap();
        }
        let mut q = SearchQuery::new(
            "old",
            E::Not(Box::new(E::Predicate(SearchPredicate {
                field: F::Mime,
                operator: O::Equal,
                value: "image/png".into(),
            }))),
        );
        q.records.path_prefix = Some("photos/".into());
        q.records.min_size = Some(5);
        q.records.status = Some(ExtractionStatus::Complete);
        let mut visual = VisualQuery::default();
        visual.load(q);
        let q = visual.query(&scan, "", RecordOrder::Path).unwrap();
        assert_eq!(q.records.min_size, Some(5));
        assert_eq!(case.count_search(&q).unwrap(), 1);
        visual.expression = E::Any(vec![]);
        assert_eq!(
            case.count_search(&visual.query(&scan, "", RecordOrder::Path).unwrap())
                .unwrap(),
            0
        );
        visual.expression = E::All(vec![]);
        assert_eq!(
            case.count_search(&visual.query(&scan, "-mime:png", RecordOrder::Path).unwrap())
                .unwrap(),
            1
        );
    }
    #[test]
    fn saved_filters_at_the_node_limit_do_not_gain_empty_wrapper_nodes() {
        let p = || {
            E::Predicate(SearchPredicate {
                field: F::Mime,
                operator: O::Exists,
                value: String::new(),
            })
        };
        let expression = E::All(vec![
            E::Any((0..63).map(|_| p()).collect()),
            E::Any((0..62).map(|_| p()).collect()),
        ]);
        let q = SearchQuery::new("test", expression);
        q.validate().unwrap();
        let mut visual = VisualQuery::default();
        visual.load(q);
        assert!(visual.query("new", "", RecordOrder::Path).is_ok());
    }
}
