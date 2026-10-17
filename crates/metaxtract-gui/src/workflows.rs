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
            return Err(io::Error::other(
                "Choose every required path / 필요한 경로를 모두 선택하세요",
            ));
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
            title: "Metadata investigation / 메타데이터 조사".into(),
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
            if ui.button("Browse / 찾아보기").clicked() {
                self.picker = Some(Picker::new(Purpose::Workflow(field)));
            }
        });
    }
    pub fn workflows_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Export and tools / 내보내기·도구");
        const ACTIONS: [&str; 18] = [
            "Export records / 레코드 내보내기",
            "Redaction profile / 비식별화 프로필",
            "Report / 보고서",
            "Generate encrypted keys / 암호화 키 생성",
            "Sign file / 파일 서명",
            "Verify file signature / 파일 서명 검증",
            "Export complete case bundle / 전체 케이스 번들",
            "Verify bundle / 번들 검증",
            "Open bundle as new case / 번들을 새 케이스로 열기",
            "Import into case / 케이스로 가져오기",
            "Verify audit / 감사 검증",
            "Parser diagnostics / 파서 진단",
            "Register public identity / 공개 서명자 등록",
            "Verify report receipt / 보고서 영수증 검증",
            "Verify original files / 원본 파일 검증",
            "Export original evidence ZIP / 원본 근거 ZIP 내보내기",
            "Verify evidence ZIP / 근거 ZIP 검증",
            "Purge metadata cache / 메타데이터 캐시 비우기",
        ];
        let previous = self.workflows.action;
        egui::ComboBox::from_id_salt("workflow-action")
            .selected_text(ACTIONS[self.workflows.action])
            .show_ui(ui, |ui| {
                for (i, label) in ACTIONS.iter().enumerate() {
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
                "Selected records only / 선택 레코드만",
            );
            ui.label("Uses the selected scan and current filter. Virtual members are not reopened as disk paths. / 선택 스캔과 현재 필터를 사용합니다. 가상 하위 항목을 디스크 경로로 열지 않습니다.");
        }
        if matches!(action, 0..=2) {
            if action != 1 {
                ui.checkbox(
                    &mut self.workflows.selected,
                    "Selected records only / 선택 레코드만",
                );
                ui.label("Uses the current record filter for the selected scan / 선택 스캔의 현재 레코드 필터를 사용합니다");
                ui.checkbox(
                    &mut self.workflows.redact,
                    "Apply redaction profile / 비식별화 프로필 적용",
                );
            }
            ui.collapsing("Custom profile JSON / 사용자 프로필 JSON",|ui| {
                ui.add(egui::TextEdit::multiline(&mut self.workflows.profile).code_editor().desired_rows(10).desired_width(f32::INFINITY));
                if ui.button("Privacy preset / 개인정보 프리셋").clicked(){self.workflows.profile=serde_json::to_string_pretty(&RedactionProfile::privacy()).expect("built-in profile");}
                ui.label("Remove/Mask rules select claim prefixes, raw namespaces/names, Privacy or AllMetadata. / 정규화 키·원본 네임스페이스·태그명·개인정보·전체 메타데이터에 제거·마스킹 규칙을 적용합니다.");
            });
        }
        match action {
            0 => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workflows.format, 0, "JSONL");
                    ui.selectable_value(&mut self.workflows.format, 1, "CSV");
                });
                self.workflow_path_row(ui, Field::Output, "Output / 출력");
            }
            1 => {
                ui.label("Preview uses the focused record in Records. Save writes the profile JSON. Source metadata is retained. / 레코드 화면의 선택 항목을 미리 봅니다. 저장하면 프로필 JSON이 생성되며 원본 메타데이터는 보존됩니다.");
                if ui
                    .add_enabled(
                        !self.busy && self.records.focused.is_some(),
                        egui::Button::new("Preview changes / 변경 미리보기"),
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
                                "Previewing redaction / 비식별화 미리보기",
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
                        Err(e) => self.status = e.to_string(),
                    }
                }
                self.workflow_path_row(ui, Field::Output, "Save profile / 프로필 저장");
            }
            2 => {
                ui.horizontal(|ui| {
                    ui.label("Title / 제목");
                    ui.text_edit_singleline(&mut self.workflows.title);
                });
                ui.horizontal(|ui| {
                    ui.label("Analyst / 분석자");
                    ui.text_edit_singleline(&mut self.workflows.analyst);
                });
                ui.horizontal(|ui| {
                    ui.label("Baseline scan (optional) / 비교 기준 스캔(선택)");
                    ui.text_edit_singleline(&mut self.workflows.baseline);
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workflows.format, 0, "HTML · EN/KR");
                    ui.selectable_value(&mut self.workflows.format, 1, "JSON");
                });
                self.workflow_path_row(ui, Field::Output, "Report / 보고서");
                ui.checkbox(
                    &mut self.workflows.signing,
                    "Sign report with encrypted key / 암호화 키로 보고서 서명",
                );
                if self.workflows.signing {
                    self.workflow_path_row(ui, Field::Receipt, "Signature receipt / 서명 영수증");
                }
            }
            3 => {
                self.workflow_path_row(ui, Field::Key, "New private key / 새 비공개 키");
                self.workflow_path_row(ui, Field::Public, "New public key / 새 공개 키");
            }
            4 => {
                self.workflow_path_row(ui, Field::Input, "File / 파일");
                self.workflow_path_row(ui, Field::Output, "Signature JSON / 서명 JSON");
            }
            5 | 13 => {
                self.workflow_path_row(ui, Field::Input, "Original file / 원본 파일");
                self.workflow_path_row(ui, Field::Receipt, "Signature JSON / 서명 JSON");
                self.workflow_path_row(ui, Field::Public, "Trusted public key / 신뢰하는 공개 키");
                ui.label(if action == 5 {
                    "Detached signature from Sign file / 파일 서명에서 만든 분리 서명"
                } else {
                    "Receipt from a signed Report / 서명된 보고서에서 만든 영수증"
                });
            }
            6 => {
                ui.label("Complete bundle includes original metadata and annotations. Redaction/filter settings do not apply. Original evidence files are not included. / 전체 번들은 원본 메타데이터·주석을 포함하며 비식별화·필터는 적용하지 않습니다. 원본 근거 파일은 포함하지 않습니다.");
                self.workflow_path_row(ui, Field::Output, "Bundle ZIP / 번들 ZIP");
                ui.checkbox(&mut self.workflows.signing, "Sign bundle / 번들 서명");
            }
            7 | 8 => {
                self.workflow_path_row(ui, Field::Input, "Bundle ZIP / 번들 ZIP");
                ui.checkbox(
                    &mut self.workflows.trusted,
                    "Require trusted signer / 신뢰하는 서명자 필수",
                );
                if self.workflows.trusted {
                    self.workflow_path_row(
                        ui,
                        Field::Public,
                        "Trusted public key / 신뢰하는 공개 키",
                    );
                } else {
                    ui.label("Checks internal consistency only; signer identity is untrusted / 내부 무결성만 검사하며 서명자는 신뢰하지 않습니다");
                }
                if action == 8 {
                    self.workflow_path_row(ui, Field::Output, "New case path / 새 케이스 경로");
                }
            }
            9 => {
                self.workflow_path_row(ui, Field::Input, "Import source / 가져올 파일");
                ui.horizontal(|ui| {
                    for (i, label) in ["JSONL", "Python case / 케이스", "Python bundle / 번들"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut self.workflows.format, i, *label);
                    }
                });
                if self.workflows.format == 2 {
                    ui.checkbox(
                        &mut self.workflows.trusted,
                        "Require trusted signer / 신뢰하는 서명자 필수",
                    );
                    if self.workflows.trusted {
                        self.workflow_path_row(
                            ui,
                            Field::Public,
                            "Trusted public key / 신뢰하는 공개 키",
                        );
                    }
                }
                ui.label("Imports append records; existing scans remain available / 기존 스캔을 보존하고 레코드를 추가합니다");
            }
            10 => {
                ui.label("Verification checks current case state and hash-chained activity. An independently retained anchor detects removed history prefixes. / 현재 상태와 해시 연결 감사 기록을 검사합니다. 별도로 보관한 앵커를 사용하면 과거 기록 잘림도 확인합니다.");
                ui.checkbox(
                    &mut self.workflows.trusted,
                    "Use retained anchor / 보관한 앵커 사용",
                );
                if self.workflows.trusted {
                    self.workflow_path_row(ui, Field::Input, "Anchor JSON / 앵커 JSON");
                }
                self.workflow_path_row(
                    ui,
                    Field::Output,
                    "Save current anchor (optional) / 현재 앵커 저장(선택)",
                );
                ui.horizontal(|ui| {
                    ui.label("Activity after sequence / 이후 활동 번호");
                    ui.add(egui::DragValue::new(&mut self.workflows.after).range(0..=i64::MAX));
                });
            }
            11 => {
                ui.label("Shows the current build, parser registry, bundled worker and stored scan engine. Record diagnostics remain in the inspector. / 현재 빌드·파서 목록·동봉 워커·저장된 스캔 엔진을 확인합니다. 레코드별 진단은 검사 화면에서 볼 수 있습니다.");
            }
            14 | 15 => {
                self.workflow_path_row(ui, Field::Input, "Original evidence root / 원본 근거 루트");
                if action == 15 {
                    self.workflow_path_row(ui, Field::Output, "Evidence ZIP / 근거 ZIP");
                    ui.label("Original files retain their embedded metadata. Export root files only. / 원본 파일 내부의 메타데이터를 유지합니다. 루트 파일을 선택하세요.");
                }
            }
            16 => {
                self.workflow_path_row(ui, Field::Input, "Evidence ZIP / 근거 ZIP");
            }
            17 => {
                ui.label("Clears cached rows only. Case, evidence and scan checkpoints are retained. / 캐시 행만 비우고 케이스·근거·체크포인트는 유지합니다.");
            }
            _ => {
                self.workflow_path_row(ui, Field::Public, "Public key / 공개 키");
            }
        }
        let uses_key = matches!(action, 3 | 4) || matches!(action, 2 | 6) && self.workflows.signing;
        if uses_key {
            if action != 3 {
                self.workflow_path_row(ui, Field::Key, "Encrypted private key / 암호화 비공개 키");
            }
            ui.horizontal(|ui| {
                ui.label("Identity name / 서명자 이름");
                ui.text_edit_singleline(&mut self.workflows.name);
            });
            ui.horizontal(|ui| {
                ui.label("Key password / 키 암호");
                ui.add(egui::TextEdit::singleline(&mut *self.workflows.password).password(true));
            });
            ui.label("Password is cleared after starting the operation / 작업 시작 후 암호 입력을 지웁니다");
        } else if action == 12 {
            ui.horizontal(|ui| {
                ui.label("Identity name / 서명자 이름");
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
                egui::Button::new("Run / 실행"),
            )
            .clicked()
        {
            self.run_workflow(ui.ctx());
        }
        ui.label("Outputs use new paths and never overwrite existing files / 출력은 새 경로를 사용하며 기존 파일을 덮어쓰지 않습니다");
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
        self.task(ctx,"Running workflow / 작업 실행 중",move || {
            let path=|f:Field|paths.get(&f.index()).ok_or_else(||io::Error::other("Required path is missing / 필요한 경로가 없습니다"));
            let case=||workspace.as_ref().ok_or_else(||io::Error::other("Open a case / 케이스를 여세요"));
            let identity=||w::identity(path(Field::Key)?,&name,&password);
            let public=||if trusted {w::read_text(path(Field::Public)?,16384).map(Some)}else{Ok(None)};
            match action {
                0=>value(a::export::export_records(case()?,query.as_ref().expect("prepared query"),path(Field::Output)?,if format==0{a::export::ExportFormat::Jsonl}else{a::export::ExportFormat::Csv},profile.as_ref(),Default::default())?),
                1=>{w::write_json(path(Field::Output)?,&profile.expect("prepared profile"))?;value(serde_json::json!({"profile_saved":true}))},
                2=> {
                    let case=case()?;let mut options=w::report_options(&title,&analyst)?;options.redaction=profile;
                    let baseline=if baseline.trim().is_empty(){None}else{Some(app::collect(case,&a::search::parse(&baseline,"")?)?)};
                    let signer=if signing{Some(identity()?)}else{None};
                    if signing {let receipt=path(Field::Receipt)?;if receipt.exists() || receipt==path(Field::Output)? {return Err(io::Error::other("Use a separate new receipt path / 별도의 새 영수증 경로를 선택하세요"));}}
                    let report=a::reporting::build_report(case,query.as_ref().expect("prepared query"),&options,baseline.as_deref(),None,Default::default())?;
                    let output=a::reporting::write_report(&report,path(Field::Output)?,if format==0 {a::reporting::ReportFormat::Html}else{a::reporting::ReportFormat::Json},signer.as_ref(),Default::default())?;
                    if signing {w::write_json(path(Field::Receipt)?,&output)?;}value(output)
                },
                3=> {
                    let key=path(Field::Key)?;let public=path(Field::Public)?;
                    if key==public || public.exists(){return Err(io::Error::other("Use separate new key paths / 서로 다른 새 키 경로를 선택하세요"));}
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
