use crate::{
    desktop::Desktop,
    files::{Picker, Purpose},
    inspectors::json_view,
};
use eframe::egui;
use metaxtract_app::{
    self as app,
    analysis::{self as a, redaction::RedactionProfile},
    storage::{self as s, CaseWorkspace},
    workflows as w,
};
use std::{io, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub enum Field {
    Input,
    Output,
    Key,
    Public,
    Receipt,
}
impl Field {
    fn index(self) -> usize {
        self as usize
    }
}
#[derive(Default)]
pub struct PathEntry {
    text: String,
    native: Option<PathBuf>,
}
impl PathEntry {
    fn path(&self) -> io::Result<PathBuf> {
        if let Some(p) = &self.native {
            return Ok(p.clone());
        }
        if self.text.trim().is_empty() {
            return Err(io::Error::other(crate::i18n::tr(
                "Choose every required path",
            )));
        }
        Ok(PathBuf::from(&self.text))
    }
}
pub struct WorkflowsUi {
    action: usize,
    paths: [PathEntry; 5],
    selected: bool,
    redact: bool,
    profile: String,
    format: usize,
    title: String,
    analyst: String,
    baseline: String,
    signing: bool,
    trusted: bool,
    name: String,
    password: app::Zeroizing<String>,
    result: Option<serde_json::Value>,
    after: i64,
}
impl Default for WorkflowsUi {
    fn default() -> Self {
        Self {
            action: 0,
            paths: Default::default(),
            selected: false,
            redact: false,
            profile: serde_json::to_string_pretty(&RedactionProfile::privacy())
                .expect("built-in profile"),
            format: 0,
            title: crate::i18n::tr("Metadata investigation").into(),
            analyst: String::new(),
            baseline: String::new(),
            signing: false,
            trusted: true,
            name: "analyst".into(),
            password: app::Zeroizing::new(String::new()),
            result: None,
            after: 0,
        }
    }
}
enum Outcome {
    Value(serde_json::Value),
    Open(Arc<CaseWorkspace>, s::CaseInfo, s::ScanHistoryPage),
    Imported(
        serde_json::Value,
        s::CaseInfo,
        s::ScanHistoryPage,
        Option<String>,
    ),
}
fn value(v: impl serde::Serialize) -> io::Result<Outcome> {
    Ok(Outcome::Value(
        serde_json::to_value(v).map_err(io::Error::other)?,
    ))
}
impl Desktop {
    pub fn workflow_path(&mut self, field: Field, path: PathBuf) {
        let p = &mut self.workflows.paths[field.index()];
        p.text = path.to_string_lossy().into_owned();
        p.native = Some(path);
    }
    fn workflow_path_row(&mut self, ui: &mut egui::Ui, field: Field, label: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            let p = &mut self.workflows.paths[field.index()];
            if ui
                .add(egui::TextEdit::singleline(&mut p.text).desired_width(400.0))
                .changed()
            {
                p.native = None;
            }
            if ui.button(crate::i18n::tr("Browse")).clicked() {
                self.picker = Some(Picker::new(Purpose::Workflow(field)));
            }
        });
    }
    pub fn workflows_view(&mut self, ui: &mut egui::Ui) {
        crate::components::heading(ui, crate::i18n::tr("Export and tools"));
        let actions: [&str; 18] = [
            crate::i18n::tr("Export records"),
            crate::i18n::tr("Redaction profile"),
            crate::i18n::tr("Report"),
            crate::i18n::tr("Generate encrypted keys"),
            crate::i18n::tr("Sign file"),
            crate::i18n::tr("Verify file signature"),
            crate::i18n::tr("Export complete case bundle"),
            crate::i18n::tr("Verify bundle"),
            crate::i18n::tr("Open bundle as new case"),
            crate::i18n::tr("Import into case"),
            crate::i18n::tr("Verify audit"),
            crate::i18n::tr("Parser diagnostics"),
            crate::i18n::tr("Register public identity"),
            crate::i18n::tr("Verify report receipt"),
            crate::i18n::tr("Verify original files"),
            crate::i18n::tr("Export original evidence ZIP"),
            crate::i18n::tr("Verify evidence ZIP"),
            crate::i18n::tr("Purge metadata cache"),
        ];
        let previous = self.workflows.action;
        egui::ComboBox::from_id_salt("workflow-action")
            .selected_text(actions[self.workflows.action])
            .show_ui(ui, |ui| {
                for (i, label) in actions.iter().enumerate() {
                    ui.selectable_value(&mut self.workflows.action, i, *label);
                }
            });
        if previous != self.workflows.action {
            self.workflows.result = None;
            self.workflows.format = 0;
            self.workflows.password = Default::default();
        }
        let action = self.workflows.action;
        ui.add_space(8.0);
        if matches!(action, 14 | 15) {
            ui.checkbox(
                &mut self.workflows.selected,
                crate::i18n::tr("Selected records only"),
            );
            ui.label(crate::i18n::tr("Uses the selected scan and current filter. Virtual members are not reopened as disk paths."));
        }
        if matches!(action, 0..=2) {
            if action != 1 {
                ui.checkbox(
                    &mut self.workflows.selected,
                    crate::i18n::tr("Selected records only"),
                );
                ui.label(crate::i18n::tr(
                    "Uses the current record filter for the selected scan",
                ));
                ui.checkbox(
                    &mut self.workflows.redact,
                    crate::i18n::tr("Apply redaction profile"),
                );
            }
            ui.collapsing(crate::i18n::tr("Custom profile JSON"),|ui| {
                ui.add(egui::TextEdit::multiline(&mut self.workflows.profile).code_editor().desired_rows(10).desired_width(f32::INFINITY));
                if ui.button(crate::i18n::tr("Privacy preset")).clicked(){self.workflows.profile=serde_json::to_string_pretty(&RedactionProfile::privacy()).expect("built-in profile");}
                ui.label(crate::i18n::tr("Remove/Mask rules select claim prefixes, raw namespaces/names, Privacy or AllMetadata."));
            });
        }
        match action {
            0 => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workflows.format, 0, "JSONL");
                    ui.selectable_value(&mut self.workflows.format, 1, "CSV");
                });
                self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Output"));
            }
            1 => {
                ui.label(crate::i18n::tr("Preview uses the focused record in Records. Save writes the profile JSON. Source metadata is retained."));
                if ui
                    .add_enabled(
                        !self.busy && self.records.focused.is_some(),
                        crate::components::button(crate::i18n::tr("Preview changes")),
                    )
                    .clicked()
                {
                    let result = self.read_profile();
                    match result {
                        Ok(profile) => {
                            let id = self.records.focused.as_ref().expect("focused record").0;
                            let case = self.case.clone().expect("open case");
                            self.task(
                                ui.ctx(),
                                crate::i18n::tr("Previewing redaction"),
                                move || {
                                    let record = case.record(id)?.ok_or_else(|| {
                                        io::Error::other("record no longer exists")
                                    })?;
                                    let alias = app::schema::EvidencePath::root(
                                        app::schema::RelativePath::parse("evidence.bin")
                                            .map_err(io::Error::other)?,
                                    );
                                    a::redaction::preview(
                                        &record,
                                        &profile,
                                        Some(alias),
                                        Default::default(),
                                    )
                                },
                                |s, preview| {
                                    s.workflows.result = serde_json::to_value(preview).ok();
                                },
                            );
                        }
                        Err(e) => self.report_error(e),
                    }
                }
                self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Save profile"));
            }
            2 => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Title"));
                    ui.text_edit_singleline(&mut self.workflows.title);
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Analyst"));
                    ui.text_edit_singleline(&mut self.workflows.analyst);
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Baseline scan (optional)"));
                    ui.text_edit_singleline(&mut self.workflows.baseline);
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workflows.format, 0, "HTML · EN/KR");
                    ui.selectable_value(&mut self.workflows.format, 1, "JSON");
                });
                self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Report"));
                ui.checkbox(
                    &mut self.workflows.signing,
                    crate::i18n::tr("Sign report with encrypted key"),
                );
                if self.workflows.signing {
                    self.workflow_path_row(
                        ui,
                        Field::Receipt,
                        crate::i18n::tr("Signature receipt"),
                    );
                }
            }
            3 => {
                self.workflow_path_row(ui, Field::Key, crate::i18n::tr("New private key"));
                self.workflow_path_row(ui, Field::Public, crate::i18n::tr("New public key"));
            }
            4 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("File"));
                self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Signature JSON"));
            }
            5 | 13 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Original file"));
                self.workflow_path_row(ui, Field::Receipt, crate::i18n::tr("Signature JSON"));
                self.workflow_path_row(ui, Field::Public, crate::i18n::tr("Trusted public key"));
                ui.label(if action == 5 {
                    crate::i18n::tr("Detached signature from Sign file")
                } else {
                    crate::i18n::tr("Receipt from a signed Report")
                });
            }
            6 => {
                ui.label(crate::i18n::tr("Complete bundle includes original metadata and annotations. Redaction/filter settings do not apply. Original evidence files are not included."));
                self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Bundle ZIP"));
                ui.checkbox(&mut self.workflows.signing, crate::i18n::tr("Sign bundle"));
            }
            7 | 8 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Bundle ZIP"));
                ui.checkbox(
                    &mut self.workflows.trusted,
                    crate::i18n::tr("Require trusted signer"),
                );
                if self.workflows.trusted {
                    self.workflow_path_row(
                        ui,
                        Field::Public,
                        crate::i18n::tr("Trusted public key"),
                    );
                } else {
                    ui.label(crate::i18n::tr(
                        "Checks internal consistency only; signer identity is untrusted",
                    ));
                }
                if action == 8 {
                    self.workflow_path_row(ui, Field::Output, crate::i18n::tr("New case path"));
                }
            }
            9 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Import source"));
                ui.horizontal(|ui| {
                    for (i, label) in [
                        "JSONL",
                        crate::i18n::tr("Python case"),
                        crate::i18n::tr("Python bundle"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut self.workflows.format, i, *label);
                    }
                });
                if self.workflows.format == 2 {
                    ui.checkbox(
                        &mut self.workflows.trusted,
                        crate::i18n::tr("Require trusted signer"),
                    );
                    if self.workflows.trusted {
                        self.workflow_path_row(
                            ui,
                            Field::Public,
                            crate::i18n::tr("Trusted public key"),
                        );
                    }
                }
                ui.label(crate::i18n::tr(
                    "Imports append records; existing scans remain available",
                ));
            }
            10 => {
                ui.label(crate::i18n::tr("Verification checks current case state and hash-chained activity. An independently retained anchor detects removed history prefixes."));
                ui.checkbox(
                    &mut self.workflows.trusted,
                    crate::i18n::tr("Use retained anchor"),
                );
                if self.workflows.trusted {
                    self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Anchor JSON"));
                }
                self.workflow_path_row(
                    ui,
                    Field::Output,
                    crate::i18n::tr("Save current anchor (optional)"),
                );
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Activity after sequence"));
                    ui.add(egui::DragValue::new(&mut self.workflows.after).range(0..=i64::MAX));
                });
            }
            11 => {
                ui.label(crate::i18n::tr("Shows the current build, parser registry, bundled worker and stored scan engine. Record diagnostics remain in the inspector."));
            }
            14 | 15 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Original evidence root"));
                if action == 15 {
                    self.workflow_path_row(ui, Field::Output, crate::i18n::tr("Evidence ZIP"));
                    ui.label(crate::i18n::tr(
                        "Original files retain their embedded metadata. Export root files only.",
                    ));
                }
            }
            16 => {
                self.workflow_path_row(ui, Field::Input, crate::i18n::tr("Evidence ZIP"));
            }
            17 => {
                ui.label(crate::i18n::tr(
                    "Clears cached rows only. Case, evidence and scan checkpoints are retained.",
                ));
            }
            _ => {
                self.workflow_path_row(ui, Field::Public, crate::i18n::tr("Public key"));
            }
        }
        let uses_key = matches!(action, 3 | 4) || matches!(action, 2 | 6) && self.workflows.signing;
        if uses_key {
            if action != 3 {
                self.workflow_path_row(ui, Field::Key, crate::i18n::tr("Encrypted private key"));
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Identity name"));
                ui.text_edit_singleline(&mut self.workflows.name);
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Key password"));
                ui.add(egui::TextEdit::singleline(&mut *self.workflows.password).password(true));
            });
            ui.label(crate::i18n::tr(
                "Password is cleared after starting the operation",
            ));
        } else if action == 12 {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Identity name"));
                ui.text_edit_singleline(&mut self.workflows.name);
            });
        }
        let requires_case = matches!(action, 0 | 2 | 6 | 9 | 10 | 12 | 14 | 15 | 17);
        let writable = matches!(action, 9 | 12);
        let switch_allowed = !self
            .info
            .as_ref()
            .is_some_and(|i| i.title != self.title || i.notes != self.notes);
        if ui
            .add_enabled(
                !self.busy
                    && !self.scan.running()
                    && (!requires_case || self.case.is_some())
                    && (!writable || !self.readonly)
                    && (action != 8 || switch_allowed),
                crate::components::button(crate::i18n::tr("Run")),
            )
            .clicked()
        {
            self.run_workflow(ui.ctx());
        }
        ui.label(crate::i18n::tr(
            "Outputs use new paths and never overwrite existing files",
        ));
        if let Some(result) = &self.workflows.result {
            ui.separator();
            json_view(ui, result);
        }
    }
    fn read_profile(&self) -> io::Result<RedactionProfile> {
        if self.workflows.profile.len() > 65536 {
            return Err(io::Error::other("Profile exceeds 64 KiB"));
        }
        let p: RedactionProfile =
            serde_json::from_str(&self.workflows.profile).map_err(io::Error::other)?;
        p.validate()?;
        Ok(p)
    }
    fn run_workflow(&mut self, ctx: &egui::Context) {
        let action = self.workflows.action;
        let prepared = (|| -> io::Result<_> {
            let query = if matches!(action, 0 | 2 | 14 | 15) {
                Some(self.active_query(self.workflows.selected)?)
            } else {
                None
            };
            let profile = if action == 1 || self.workflows.redact && matches!(action, 0 | 2) {
                Some(self.read_profile()?)
            } else {
                None
            };
            let mut paths = std::collections::BTreeMap::new();
            for f in [
                Field::Input,
                Field::Output,
                Field::Key,
                Field::Public,
                Field::Receipt,
            ] {
                let entry = &self.workflows.paths[f.index()];
                if entry.native.is_some() || !entry.text.is_empty() {
                    paths.insert(f.index(), entry.path()?);
                }
            }
            Ok((query, profile, paths))
        })();
        let (query, profile, paths) = match prepared {
            Ok(v) => v,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let workspace = self.case.clone();
        let scan = self.active_scan.clone();
        let title = self.workflows.title.clone();
        let analyst = self.workflows.analyst.clone();
        let baseline = self.workflows.baseline.clone();
        let format = self.workflows.format;
        let name = self.workflows.name.clone();
        let signing = self.workflows.signing;
        let trusted = self.workflows.trusted;
        let after = self.workflows.after;
        let password = std::mem::take(&mut self.workflows.password);
        self.workflows.result = None;
        self.task(ctx,crate::i18n::tr("Running workflow"),move || {
            let path=|f:Field|paths.get(&f.index()).ok_or_else(||io::Error::other(crate::i18n::tr("Required path is missing")));
            let case=||workspace.as_ref().ok_or_else(||io::Error::other(crate::i18n::tr("Open a case")));
            let identity=||w::identity(path(Field::Key)?,&name,&password);
            let public=||if trusted {w::read_text(path(Field::Public)?,16384).map(Some)}else{Ok(None)};
            match action {
                0=>value(a::export::export_records(case()?,query.as_ref().expect("prepared query"),path(Field::Output)?,if format==0{a::export::ExportFormat::Jsonl}else{a::export::ExportFormat::Csv},profile.as_ref(),Default::default())?),
                1=>{w::write_json(path(Field::Output)?,&profile.expect("prepared profile"))?;value(serde_json::json!({"profile_saved":true}))},
                2=> {
                    let case=case()?;let mut options=w::report_options(&title,&analyst)?;options.redaction=profile;
                    let baseline=if baseline.trim().is_empty(){None}else{Some(app::collect(case,&a::search::parse(&baseline,"")?)?)};
                    let signer=if signing{Some(identity()?)}else{None};
                    if signing {let receipt=path(Field::Receipt)?;if receipt.exists() || receipt==path(Field::Output)? {return Err(io::Error::other(crate::i18n::tr("Use a separate new receipt path")));}}
                    let report=a::reporting::build_report(case,query.as_ref().expect("prepared query"),&options,baseline.as_deref(),None,Default::default())?;
                    let output=a::reporting::write_report(&report,path(Field::Output)?,if format==0 {a::reporting::ReportFormat::Html}else{a::reporting::ReportFormat::Json},signer.as_ref(),Default::default())?;
                    if signing {w::write_json(path(Field::Receipt)?,&output)?;}value(output)
                },
                3=> {
                    let key=path(Field::Key)?;let public=path(Field::Public)?;
                    if key==public || public.exists(){return Err(io::Error::other(crate::i18n::tr("Use separate new key paths")));}
                    let id=s::SigningIdentity::generate(&name)?;id.save_encrypted(key,&password)?;w::write_bytes(public,id.public_pem()?.as_bytes())?;
                    value(serde_json::json!({"name":id.name(),"fingerprint":id.fingerprint(),"encrypted_private_key_saved":true}))
                },
                4=>{let bytes=app::read_bytes(path(Field::Input)?,256*1024*1024)?;let sig=identity()?.sign_manifest(&bytes);w::write_json(path(Field::Output)?,&sig)?;value(sig)},
                5=>{let bytes=app::read_bytes(path(Field::Input)?,256*1024*1024)?;let sig:s::ManifestSignature=w::read_json(path(Field::Receipt)?)?;let pem=w::read_text(path(Field::Public)?,16384)?;value(serde_json::json!({"verified":true,"fingerprint":s::verify_manifest(&bytes,&sig,&pem)?}))},
                6=>{let id=if signing{Some(identity()?)}else{None};value(case()?.export_snapshot(path(Field::Output)?,id.as_ref(),&Default::default())?)},
                7=>value(s::verify_snapshot(path(Field::Input)?,public()?.as_deref(),&Default::default())?),
                8=> {let opened=Arc::new(CaseWorkspace::open_snapshot(path(Field::Input)?,path(Field::Output)?,s::AccessMode::ReadWrite,public()?.as_deref(),&Default::default())?);let info=opened.info()?;let history=opened.scan_history(None,128)?;Ok(Outcome::Open(opened,info,history))},
                9=>{let case=case()?;let limits=s::ImportLimits::default();let result=match format{0=>case.import_jsonl(path(Field::Input)?,&limits)?,1=>case.import_python_case(path(Field::Input)?,&limits)?,_=>case.import_python_bundle(path(Field::Input)?,public()?.as_deref(),&limits)?};let scan=result.scans.first().cloned();Ok(Outcome::Imported(serde_json::to_value(result).map_err(io::Error::other)?,case.info()?,case.scan_history(None,128)?,scan))},
                10=>{let case=case()?;let expected=if trusted {Some(w::read_json::<s::AuditAnchor>(path(Field::Input)?)?)}else{None};let verified=case.verify_integrity(expected.as_ref())?;let head=case.audit_head()?;if let Some(p)=paths.get(&Field::Output.index()){w::write_json(p,&head)?;}let events=case.audit_events(after,128)?;value(serde_json::json!({"integrity":verified,"anchor":head,"events":events,"next_after":events.last().map(|e|e.sequence),"page_limit":128}))},
                11=>value(w::diagnostics(workspace.as_deref(),scan.as_deref())?),
                13=>{let receipt:a::reporting::ReportOutput=w::read_json(path(Field::Receipt)?)?;let pem=w::read_text(path(Field::Public)?,16384)?;let input=std::fs::File::open(path(Field::Input)?)?;value(serde_json::json!({"verified":true,"fingerprint":a::reporting::verify_report(input,&receipt,&pem,Default::default())?}))},
                14=>value(w::verify_originals(case()?,query.as_ref().expect("prepared query"),path(Field::Input)?)?),
                15=>value(w::export_evidence(case()?,query.as_ref().expect("prepared query"),path(Field::Input)?,path(Field::Output)?)?),
                16=>value(a::export::verify_original_evidence(path(Field::Input)?,Default::default())?),
                17=>value(serde_json::json!({"purged_entries":app::core::scan::RecordCache::purge_existing(w::cache_path(case()?.path()))?})),
                _=>{let pem=w::read_text(path(Field::Public)?,16384)?;let registered=case()?.register_public_identity(&name,&pem)?;value(serde_json::json!({"registered":registered,"identities":case()?.identities(None,128)?}))}
            }
        },|s,out|match out {
            Outcome::Value(v)=>s.workflows.result=Some(v),
            Outcome::Open(case,info,history)=>s.loaded(case,info,history),
            Outcome::Imported(v,info,history,scan)=>{s.info=Some(info);s.scans=history.items;s.history_next=history.next;s.active_scan=scan;s.records=Default::default();s.analysis.metrics_loaded=false;s.workflows.result=Some(v);}
        });
    }
}
