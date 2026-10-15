use crate::{
    AnalysisLimits, LimitedWriter, diff, duplicates, export, findings, geo, hex, invalid,
    normalize, provenance, redaction, schema::*, validation,
};
use metaxtract_storage::{
    AuditAnchor, CaseWorkspace, ManifestSignature, SearchQuery, SigningIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ReportLanguage {
    English,
    Korean,
    Bilingual,
}
impl ReportLanguage {
    fn text(self, en: &str, kr: &str) -> String {
        match self {
            Self::English => en.into(),
            Self::Korean => kr.into(),
            Self::Bilingual => format!("{en} / {kr}"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportOptions {
    pub title: String,
    pub analyst: String,
    pub generated_unix_seconds: i64,
    pub language: ReportLanguage,
    pub expected_anchor: Option<AuditAnchor>,
    pub redaction: Option<redaction::RedactionProfile>,
    pub cluster_radius_meters: f64,
    pub track_groups: BTreeMap<String, String>,
}
impl ReportOptions {
    pub fn new(
        title: impl Into<String>,
        analyst: impl Into<String>,
        generated_unix_seconds: i64,
    ) -> Self {
        Self {
            title: title.into(),
            analyst: analyst.into(),
            generated_unix_seconds,
            language: ReportLanguage::Bilingual,
            expected_anchor: None,
            redaction: None,
            cluster_radius_meters: 100.0,
            track_groups: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportRecord {
    pub ordinal: u64,
    pub path: EvidencePath,
    pub original_record_sha256: Option<String>,
    pub metadata: Value,
    pub original_claims: Vec<NormalizedClaim>,
    pub analyzed_claims: Vec<NormalizedClaim>,
    pub lineage: Vec<provenance::FieldLineage>,
    pub findings: Vec<findings::Finding>,
    pub normalization_issues: Vec<normalize::NormalizationIssue>,
    pub validation_issues: Vec<validation::ValidationIssue>,
    pub encodings: Vec<normalize::EncodingInfo>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportCoverage {
    pub full_scan_records: u64,
    pub selected_records: u64,
    pub excluded_records: u64,
    pub status_counts: BTreeMap<String, u64>,
    pub findings_by_severity: BTreeMap<String, u64>,
    pub visual_candidates: u64,
    pub visual_records_with_previews: u64,
    pub visual_unavailable: Vec<EvidencePath>,
    pub invalid_locations: Vec<geo::GeoIssue>,
    pub content_similarity_status: String,
    pub content_fingerprint_records: u64,
    pub binary_presentation: String,
    pub original_evidence_included: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InvestigationReport {
    pub format: String,
    pub title: String,
    pub analyst: String,
    pub generated_unix_seconds: i64,
    pub language: ReportLanguage,
    pub case_id: String,
    pub case_revision: u64,
    pub scan_id: String,
    pub scan: metaxtract_storage::ScanInfo,
    pub anchor: AuditAnchor,
    pub case_state_verified: bool,
    pub independently_anchored_prefix: bool,
    pub query: Option<SearchQuery>,
    pub selection_sha256: String,
    pub redaction_profile: Option<String>,
    pub coverage: ReportCoverage,
    pub records: Vec<ReportRecord>,
    pub timeline: Vec<provenance::TimelineEvent>,
    pub exact_duplicates: Vec<duplicates::ExactGroup>,
    pub visually_similar: Vec<duplicates::SimilarityMatch>,
    pub content_similar: Vec<duplicates::SimilarityMatch>,
    pub locations: Vec<geo::GeoSample>,
    pub clusters: Vec<geo::GeoCluster>,
    pub tracks: geo::Tracks,
    pub comparison: Option<diff::ScanDiff>,
    pub limitations: Vec<String>,
}
fn prepare(
    mut record: ExtractionRecord,
    limits: AnalysisLimits,
) -> io::Result<(
    ExtractionRecord,
    normalize::Normalization,
    validation::Validation,
)> {
    let normalized = normalize::normalize(&record, limits)?;
    record.claims.extend(normalized.claims.iter().cloned());
    let checked = validation::reconcile(&record, limits)?;
    record.claims = checked.claims.clone();
    for conflict in &checked.conflicts {
        if !record
            .conflicts
            .iter()
            .any(|c| c.key == conflict.key && c.alternatives == conflict.alternatives)
        {
            record.conflicts.push(conflict.clone());
        }
    }
    Ok((record, normalized, checked))
}
pub fn build_report(
    case: &CaseWorkspace,
    query: &SearchQuery,
    options: &ReportOptions,
    baseline: Option<&[ExtractionRecord]>,
    fingerprints: Option<&[duplicates::ContentFingerprint]>,
    limits: AnalysisLimits,
) -> io::Result<InvestigationReport> {
    if options.title.is_empty()
        || options.title.len() > 4096
        || options.analyst.len() > 4096
        || options.title.contains('\0')
        || options.analyst.contains('\0')
    {
        return Err(invalid("invalid report title or analyst"));
    }
    if let Some(p) = &options.redaction {
        p.validate()?;
    }
    let (info, anchor) = export::stamp(case)?;
    let mut scan = case
        .scan_info(&query.records.scan_id)?
        .ok_or_else(|| invalid("report scan does not exist"))?;
    if options.redaction.is_some() {
        scan.source = "[withheld]".into();
        scan.imported_from = None;
    }
    if let Some(expected) = &options.expected_anchor {
        case.verify_integrity(Some(expected))?;
    }
    let full_scan_records = case.count_records(&metaxtract_storage::RecordQuery::new(
        &query.records.scan_id,
    ))?;
    let selection_sha256 = hex(&Sha256::digest(
        serde_json::to_vec(query).map_err(io::Error::other)?,
    ));
    let mut fingerprint_index = BTreeMap::new();
    if let Some(fingerprints) = fingerprints {
        if fingerprints.len() > limits.max_records {
            return Err(invalid("report fingerprint input budget exceeded"));
        }
        for fp in fingerprints {
            if fingerprint_index
                .insert(fp.path().to_string(), fp)
                .is_some()
            {
                return Err(invalid(
                    "duplicate content fingerprint for one evidence path",
                ));
            }
        }
    }
    let mut records = vec![];
    let mut analyzed = vec![];
    let mut visual = vec![];
    let mut visual_unavailable = vec![];
    let mut eligible = 0;
    let mut visual_records = 0;
    let mut statuses = BTreeMap::new();
    let mut severities = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    let mut used_fingerprints = vec![];
    let mut retained_bytes = 0usize;
    let selected_records = export::selected(case, query, limits, |ordinal, original| {
        let path = original.source.path.to_string();
        if let Some(fp) = fingerprint_index.get(&path) {
            if !fp.matches_record(&original) {
                return Err(invalid(
                    "content fingerprint does not match selected evidence",
                ));
            }
            used_fingerprints.push((*fp).clone());
        }
        let original_record_sha256 = if options.redaction.as_ref().is_some_and(|p| p.omit_digests) {
            None
        } else {
            Some(hex(&Sha256::digest(
                serde_json::to_vec(&original).map_err(io::Error::other)?,
            )))
        };
        let alias = EvidencePath::root(
            RelativePath::parse(format!("evidence-{ordinal}.bin")).map_err(io::Error::other)?,
        );
        let record = if let Some(p) = &options.redaction {
            redaction::preview(&original, p, Some(alias.clone()), limits)?.record
        } else {
            original
        };
        aliases.insert(path, record.source.path.clone());
        let original_claims = record.claims.clone();
        let (record, normalized, checked) = prepare(record, limits)?;
        *statuses.entry(format!("{:?}", record.status)).or_insert(0) += 1;
        let found = findings::classify(&record, limits)?;
        for f in &found {
            *severities.entry(format!("{:?}", f.severity)).or_insert(0) += 1;
        }
        let candidate = record
            .source
            .detected_media_type
            .as_deref()
            .or(record.source.declared_media_type.as_deref())
            .is_some_and(|m| m.starts_with("image/") || m.starts_with("video/"));
        let previews = if options
            .redaction
            .as_ref()
            .is_some_and(|p| p.strip_encoded_payloads)
        {
            vec![]
        } else {
            duplicates::visual_fingerprints(&record, 1, limits)?
        };
        if candidate {
            eligible += 1;
            if previews.is_empty() {
                visual_unavailable.push(record.source.path.clone());
            } else {
                visual_records += 1;
            }
        }
        visual.extend(previews);
        let presentation = ReportRecord {
            ordinal,
            path: record.source.path.clone(),
            original_record_sha256,
            metadata: diff::view(&record),
            original_claims,
            analyzed_claims: record.claims.clone(),
            lineage: provenance::field_lineage(&record, limits)?,
            findings: found,
            normalization_issues: normalized.issues,
            validation_issues: checked.issues,
            encodings: normalized.encodings,
        };
        let bytes = serde_json::to_vec(&presentation)
            .map_err(io::Error::other)?
            .len();
        retained_bytes = retained_bytes
            .saturating_add(bytes)
            .saturating_add(serde_json::to_vec(&record).map_err(io::Error::other)?.len());
        if retained_bytes > limits.max_bytes {
            return Err(invalid("report retained metadata budget exceeded"));
        }
        records.push(presentation);
        analyzed.push(record);
        Ok(())
    })?;
    let mut content_similar = duplicates::near(&used_fingerprints, 800, limits)?;
    for pair in &mut content_similar {
        pair.first = aliases[&pair.first.to_string()].clone();
        pair.second = aliases[&pair.second.to_string()].clone();
    }
    let geographic = geo::location_analysis(&analyzed, limits)?;
    let locations = geographic.samples;
    let clusters = geo::clusters(&locations, options.cluster_radius_meters, 2, limits)?;
    let track_groups = options
        .track_groups
        .iter()
        .filter_map(|(p, g)| aliases.get(p).map(|alias| (alias.to_string(), g.clone())))
        .collect();
    let tracks = geo::tracks(&locations, &track_groups, 86400, 100.0, limits)?;
    let comparison = if let Some(baseline) = baseline {
        if baseline.len() > limits.max_records {
            return Err(invalid("report baseline record budget exceeded"));
        }
        let mut projected = vec![];
        for (i, r) in baseline.iter().enumerate() {
            let alias =
                aliases
                    .get(&r.source.path.to_string())
                    .cloned()
                    .unwrap_or(EvidencePath::root(
                        RelativePath::parse(format!("baseline-{}.bin", i + 1))
                            .map_err(io::Error::other)?,
                    ));
            let record = if let Some(p) = &options.redaction {
                redaction::preview(r, p, Some(alias), limits)?.record
            } else {
                r.clone()
            };
            projected.push(prepare(record, limits)?.0);
        }
        Some(diff::compare_scans(&projected, &analyzed, limits)?)
    } else {
        None
    };
    let coverage = ReportCoverage {
        full_scan_records,
        selected_records,
        excluded_records: full_scan_records.saturating_sub(selected_records),
        status_counts: statuses,
        findings_by_severity: severities,
        visual_candidates: eligible,
        visual_records_with_previews: visual_records,
        visual_unavailable,
        invalid_locations: geographic.issues,
        content_similarity_status: if fingerprints.is_none() {
            "not_requested"
        } else if used_fingerprints.len() as u64 == selected_records {
            "provided_for_all_selected"
        } else {
            "partial_coverage"
        }
        .into(),
        content_fingerprint_records: used_fingerprints.len() as u64,
        binary_presentation: "binary length and SHA-256; full bytes remain in the case".into(),
        original_evidence_included: false,
    };
    let limitations=vec![
        "Findings are evidence-linked rules and heuristics, not proof of forgery, authorship or malware. / 발견 사항은 근거가 연결된 규칙·추정이며 위조·작성자·악성코드를 확정하지 않습니다.".into(),
        "Missing zones and partial dates remain unresolved; filesystem times can change during copying. / 시간대 미확인·부분 날짜는 미해결이며 복사 과정에서 파일시스템 시각이 바뀔 수 있습니다.".into(),
        "Visual matching uses available unoriented RGB previews; unavailable decoders and other frames are coverage gaps. / 시각 비교는 제공된 RGB 미리보기 기준이며 미지원 디코더·다른 프레임에는 한계가 있습니다.".into(),
        "Case hash-chain consistency requires an independent trusted anchor or signature for authentication. / 케이스 해시 체인의 일관성과 별개로 인증에는 독립적으로 신뢰한 기준점이나 서명이 필요합니다.".into(),
        "A redaction profile affects selected metadata only; review remaining fields and user-supplied report text. / 비식별화는 선택한 메타데이터에 적용되므로 나머지 필드와 직접 입력한 보고서 문구도 검토해야 합니다.".into(),
    ];
    let out = InvestigationReport {
        format: "metaxtract-investigation-report-v1".into(),
        title: options.title.clone(),
        analyst: options.analyst.clone(),
        generated_unix_seconds: options.generated_unix_seconds,
        language: options.language,
        case_id: info.case_id.clone(),
        case_revision: info.revision,
        scan_id: query.records.scan_id.clone(),
        scan,
        anchor: anchor.clone(),
        case_state_verified: true,
        independently_anchored_prefix: options.expected_anchor.is_some(),
        query: if options.redaction.is_some() {
            None
        } else {
            Some(query.clone())
        },
        selection_sha256,
        redaction_profile: options.redaction.as_ref().map(|p| p.name.clone()),
        timeline: provenance::timeline(&analyzed, limits)?,
        exact_duplicates: duplicates::exact(&analyzed, limits)?,
        visually_similar: duplicates::visually_similar(&visual, 8, limits)?,
        content_similar,
        locations,
        clusters,
        tracks,
        coverage,
        records,
        comparison,
        limitations,
    };
    serde_json::to_writer(LimitedWriter::new(io::sink(), limits.max_bytes), &out)
        .map_err(io::Error::other)?;
    export::unchanged(case, &info, &anchor)?;
    Ok(out)
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ReportFormat {
    Html,
    Json,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportOutput {
    pub bytes: u64,
    pub sha256: String,
    pub signed_manifest: Value,
    pub signature: Option<ManifestSignature>,
}
pub fn write_report(
    report: &InvestigationReport,
    destination: impl AsRef<Path>,
    format: ReportFormat,
    signer: Option<&SigningIdentity>,
    limits: AnalysisLimits,
) -> io::Result<ReportOutput> {
    let (_, bytes, sha256) = export::atomic_output(destination.as_ref(), |file| {
        let mut writer = LimitedWriter::new(file, limits.max_bytes);
        match format {
            ReportFormat::Json => {
                serde_json::to_writer_pretty(&mut writer, report).map_err(io::Error::other)?
            }
            ReportFormat::Html => write_html(report, &mut writer)?,
        }
        writer.flush()
    })?;
    let signed_manifest = serde_json::json!({"format":"metaxtract-report-digest-v1","report_format":format,"bytes":bytes,"sha256":sha256,"case_id":report.case_id,"anchor":report.anchor});
    let signature = signer
        .map(|s| serde_json::to_vec(&signed_manifest).map(|bytes| s.sign_manifest(&bytes)))
        .transpose()
        .map_err(io::Error::other)?;
    Ok(ReportOutput {
        bytes,
        sha256,
        signed_manifest,
        signature,
    })
}
fn write_html(report: &InvestigationReport, w: &mut impl Write) -> io::Result<()> {
    let e = geo::escape_xml;
    let t = |en: &str, kr: &str| e(&report.language.text(en, kr));
    let lang = if matches!(
        report.language,
        ReportLanguage::Korean | ReportLanguage::Bilingual
    ) {
        "ko"
    } else {
        "en"
    };
    write!(
        w,
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>{}</title><style>body{{font:15px/1.6 system-ui,sans-serif;color:#172033;background:#f4f6fa;margin:0}}main{{max-width:1100px;margin:32px auto;padding:32px;background:white}}h1,h2{{color:#153e75}}h2{{border-top:2px solid #e2e8f0;padding-top:20px}}table{{border-collapse:collapse;width:100%;margin:16px 0}}td,th{{border:1px solid #d7deea;padding:8px;text-align:left;vertical-align:top;overflow-wrap:anywhere}}th{{background:#edf2fa}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;background:#f5f7fa;padding:16px}}code{{overflow-wrap:anywhere}}summary{{cursor:pointer;font-weight:600}}.muted{{color:#526174}}@media print{{body{{background:white}}main{{margin:0;padding:0}}pre{{font-size:9px}}}}</style></head><body><main><h1>{}</h1><p class=\"muted\">{}</p>",
        e(&report.title),
        e(&report.title),
        t(
            "Evidence and metadata investigation report",
            "증거·메타데이터 조사 보고서"
        )
    )?;
    write!(
        w,
        "<table><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td><code>{}</code></td></tr><tr><th>{}</th><td>{}</td></tr><tr><th>{}</th><td>{} / {}</td></tr><tr><th>{}</th><td><code>{}</code></td></tr></table>",
        t("Analyst", "분석자"),
        e(&report.analyst),
        t("Case", "케이스"),
        e(&report.case_id),
        t("Generated UTC epoch seconds", "생성 시각 UTC 초"),
        report.generated_unix_seconds,
        t("Selected / full scan", "선택 / 전체 스캔"),
        report.coverage.selected_records,
        report.coverage.full_scan_records,
        t("Audit head", "감사 기준점"),
        e(&report.anchor.hash)
    )?;
    write!(
        w,
        "<h2>{}</h2><ul>",
        t("Scope and limitations", "범위와 한계")
    )?;
    for limitation in &report.limitations {
        write!(w, "<li>{}</li>", e(limitation))?;
    }
    write!(
        w,
        "</ul><pre>{}</pre>",
        e(&serde_json::to_string_pretty(&report.coverage).map_err(io::Error::other)?)
    )?;
    write!(
        w,
        "<h2>{}</h2>",
        t("Evidence records and findings", "증거 레코드와 발견 사항")
    )?;
    for record in &report.records {
        let path = record.path.to_string();
        write!(
            w,
            "<h3>{}. {}</h3><table><tr><th>{}</th><th>{}</th><th>{}</th></tr>",
            record.ordinal,
            e(&path),
            t("Rule / severity", "규칙 / 심각도"),
            t("Explanation", "설명"),
            t("Evidence links", "근거 연결")
        )?;
        for f in &record.findings {
            write!(
                w,
                "<tr><td>{} / {:?}</td><td>{}</td><td>{}</td></tr>",
                e(&f.rule),
                f.severity,
                e(&f.explanation),
                e(&format!(
                    "observations {:?}; claims {:?}",
                    f.observations, f.claims
                ))
            )?;
        }
        write!(
            w,
            "</table><details><summary>{}</summary><pre>{}</pre></details>",
            t(
                "Metadata, claims and full field lineage",
                "메타데이터·해석·전체 필드 출처"
            ),
            e(&serde_json::to_string_pretty(record).map_err(io::Error::other)?)
        )?;
    }
    for (en, kr, value) in [
        (
            "Timeline",
            "타임라인",
            serde_json::to_value(&report.timeline),
        ),
        (
            "Exact duplicates",
            "동일 파일",
            serde_json::to_value(&report.exact_duplicates),
        ),
        (
            "Visual similarity",
            "시각 유사도",
            serde_json::to_value(&report.visually_similar),
        ),
        (
            "Content similarity",
            "내용 유사도",
            serde_json::to_value(&report.content_similar),
        ),
        (
            "Locations and clusters",
            "위치·군집",
            Ok(
                serde_json::json!({"locations":report.locations,"clusters":report.clusters,"tracks":report.tracks}),
            ),
        ),
        (
            "Comparison",
            "비교",
            serde_json::to_value(&report.comparison),
        ),
    ] {
        write!(
            w,
            "<h2>{}</h2><pre>{}</pre>",
            t(en, kr),
            e(
                &serde_json::to_string_pretty(&value.map_err(io::Error::other)?)
                    .map_err(io::Error::other)?
            )
        )?;
    }
    write!(
        w,
        "<footer><p>{}</p><code>{}</code></footer></main></body></html>",
        t("Selection digest", "선택 조건 해시"),
        e(&report.selection_sha256)
    )
}
