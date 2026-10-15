use crate::args::Args;
use metaxtract_app::{self as app, analysis as a, storage as s, workflows as w};
use serde_json::{Value, json};
use std::io::{self, Read};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn value(v: impl serde::Serialize) -> io::Result<Value> {
    serde_json::to_value(v).map_err(io::Error::other)
}
fn query(args: &Args, scan: &str) -> io::Result<s::SearchQuery> {
    let mut q = a::search::parse(scan, &args.flag("query").unwrap_or_default())?;
    q.records.order = match args.flag("order").as_deref().unwrap_or("path") {
        "path" => s::RecordOrder::Path,
        "size" => s::RecordOrder::Size,
        "insertion" => s::RecordOrder::Insertion,
        _ => return Err(invalid("order must be path, size or insertion")),
    };
    Ok(q)
}
fn password(args: &Args) -> io::Result<app::Zeroizing<String>> {
    let password = if args.flags.contains_key("password-stdin") {
        let mut input = app::Zeroizing::new(String::new());
        io::stdin().take(16 * 1024 + 1).read_to_string(&mut input)?;
        if input.len() > 16 * 1024 {
            return Err(invalid("password input exceeds 16 KiB"));
        }
        while input.ends_with(['\n', '\r']) {
            input.pop();
        }
        input
    } else {
        app::Zeroizing::new(
            std::env::var("METAXTRACT_KEY_PASSWORD")
                .map_err(|_| invalid("use --password-stdin or METAXTRACT_KEY_PASSWORD"))?,
        )
    };
    if password.is_empty() || password.len() > 16 * 1024 {
        return Err(invalid("empty or oversized password"));
    }
    Ok(password)
}
fn signer(args: &Args) -> io::Result<Option<s::SigningIdentity>> {
    args.flag_path("key")
        .map(|path| {
            w::identity(
                &path,
                &args.flag("name").unwrap_or_else(|| "analyst".into()),
                &password(args)?,
            )
        })
        .transpose()
}
fn trust(args: &Args) -> io::Result<Option<String>> {
    args.flag_path("public-key")
        .map(|p| w::read_text(&p, 16 * 1024))
        .transpose()
}
pub fn run(args: &Args) -> io::Result<Value> {
    match args.word(0)?.as_str() {
        "preview" => {
            let case = app::open_case(&args.path(1)?, false)?;
            let id = args
                .word(2)?
                .parse()
                .map_err(|_| invalid("invalid record id"))?;
            let record = case
                .record(id)?
                .ok_or_else(|| invalid("record not found"))?;
            value(app::preview::load(&record, &args.path(3)?)?)
        }
        "search" => {
            let case = app::open_case(&args.path(1)?, args.flags.contains_key("save"))?;
            let q = query(args, &args.word(2)?)?;
            if let Some(name) = args.flag("save") {
                case.save_filter(&name, &s::FilterQuery::Structured(q.clone()))?;
            }
            let cursor = args
                .flag("cursor")
                .map(|c| serde_json::from_str::<s::PageCursor>(&c).map_err(io::Error::other))
                .transpose()?;
            let limit = args
                .flag("limit")
                .map(|n| {
                    n.parse::<usize>()
                        .map_err(|_| invalid("invalid page limit"))
                })
                .transpose()?
                .unwrap_or(128);
            let page = case.search_records(&q, cursor.as_ref(), limit)?;
            Ok(json!({"items":page.items,"next":page.next,"total":case.count_search(&q)?}))
        }
        "export" | "redact" => {
            let redact = args.word(0)? == "redact";
            let case = app::open_case(&args.path(1)?, false)?;
            let q = query(args, &args.word(2)?)?;
            let profile = w::profile(args.flag_path("profile").as_deref(), redact)?;
            if let Some(id) = args.flag("preview") {
                let id = id.parse().map_err(|_| invalid("invalid record id"))?;
                let record = case.record(id)?.ok_or_else(|| invalid("unknown record"))?;
                let alias = app::schema::EvidencePath::root(
                    app::schema::RelativePath::parse("evidence.bin").map_err(io::Error::other)?,
                );
                return value(a::redaction::preview(
                    &record,
                    profile
                        .as_ref()
                        .ok_or_else(|| invalid("preview requires a profile"))?,
                    Some(alias),
                    Default::default(),
                )?);
            }
            let format = match args.flag("format").as_deref().unwrap_or("jsonl") {
                "csv" => a::export::ExportFormat::Csv,
                "jsonl" => a::export::ExportFormat::Jsonl,
                _ => return Err(invalid("export format must be csv or jsonl")),
            };
            value(a::export::export_records(
                &case,
                &q,
                args.path(3)?,
                format,
                profile.as_ref(),
                Default::default(),
            )?)
        }
        "report" => {
            let case = app::open_case(&args.path(1)?, false)?;
            let q = query(args, &args.word(2)?)?;
            let mut options = w::report_options(
                &args
                    .flag("title")
                    .unwrap_or_else(|| "Metadata investigation / 메타데이터 조사".into()),
                &args.flag("analyst").unwrap_or_default(),
            )?;
            options.redaction = w::profile(args.flag_path("profile").as_deref(), false)?;
            options.expected_anchor = args
                .flag_path("trusted-anchor")
                .map(|p| w::read_json(&p))
                .transpose()?;
            let baseline = args
                .flag("baseline")
                .map(|id| app::collect(&case, &a::search::parse(&id, "")?))
                .transpose()?;
            let format = match args.flag("format").as_deref().unwrap_or("html") {
                "html" => a::reporting::ReportFormat::Html,
                "json" => a::reporting::ReportFormat::Json,
                _ => return Err(invalid("report format must be html or json")),
            };
            let identity = signer(args)?;
            let receipt = args.flag_path("receipt");
            if identity.is_some() && receipt.is_none() {
                return Err(invalid(
                    "signed reports require --receipt PATH to retain the signed manifest",
                ));
            }
            if receipt.as_ref().is_some_and(|p| p.exists()) {
                return Err(invalid("receipt destination exists"));
            }
            let report = a::reporting::build_report(
                &case,
                &q,
                &options,
                baseline.as_deref(),
                None,
                Default::default(),
            )?;
            let out = a::reporting::write_report(
                &report,
                args.path(3)?,
                format,
                identity.as_ref(),
                Default::default(),
            )?;
            if let Some(path) = receipt {
                w::write_json(&path, &out)?;
            }
            value(out)
        }
        "keygen" => {
            let public = args.path(2)?;
            if public.exists() {
                return Err(invalid("public key destination exists"));
            }
            let identity = s::SigningIdentity::generate(
                &args.flag("name").unwrap_or_else(|| "analyst".into()),
            )?;
            identity.save_encrypted(args.path(1)?, &password(args)?)?;
            w::write_bytes(&public, identity.public_pem()?.as_bytes())?;
            Ok(
                json!({"name":identity.name(),"fingerprint":identity.fingerprint(),"public_key":public}),
            )
        }
        "sign" => {
            let bytes = app::read_bytes(&args.path(1)?, 256 * 1024 * 1024)?;
            let signature = signer(args)?
                .ok_or_else(|| invalid("sign requires --key PATH"))?
                .sign_manifest(&bytes);
            w::write_json(&args.path(2)?, &signature)?;
            value(signature)
        }
        "verify-signature" => {
            let bytes = app::read_bytes(&args.path(1)?, 256 * 1024 * 1024)?;
            let signature = w::read_json(&args.path(2)?)?;
            let public = w::read_text(&args.path(3)?, 16 * 1024)?;
            Ok(
                json!({"verified":true,"fingerprint":s::verify_manifest(&bytes,&signature,&public)?}),
            )
        }
        "bundle" => {
            let public = trust(args)?;
            match args.word(1)?.as_str() {
                "export" => value(app::open_case(&args.path(2)?, false)?.export_snapshot(
                    args.path(3)?,
                    signer(args)?.as_ref(),
                    &Default::default(),
                )?),
                "verify" => value(s::verify_snapshot(
                    args.path(2)?,
                    public.as_deref(),
                    &Default::default(),
                )?),
                "open" => value(
                    s::CaseWorkspace::open_snapshot(
                        args.path(2)?,
                        args.path(3)?,
                        s::AccessMode::ReadWrite,
                        public.as_deref(),
                        &Default::default(),
                    )?
                    .info()?,
                ),
                _ => Err(invalid("bundle expects export, verify or open")),
            }
        }
        "import" => {
            let case = app::open_case(&args.path(1)?, true)?;
            let input = args.path(2)?;
            let limits = s::ImportLimits::default();
            value(match args.flag("kind").as_deref().unwrap_or("jsonl") {
                "jsonl" => case.import_jsonl(input, &limits)?,
                "python" => case.import_python_case(input, &limits)?,
                "python-bundle" => {
                    case.import_python_bundle(input, trust(args)?.as_deref(), &limits)?
                }
                _ => {
                    return Err(invalid(
                        "import kind must be jsonl, python or python-bundle",
                    ));
                }
            })
        }
        "analyze" => {
            let case = app::open_case(&args.path(1)?, false)?;
            let q = query(args, &args.word(2)?)?;
            let records = app::collect(&case, &q)?;
            let limits = a::AnalysisLimits::default();
            match args.flag("view").as_deref().unwrap_or("dashboard") {
                "timeline" => value(a::provenance::timeline(&records, limits)?),
                "locations" => value(a::geo::location_analysis(&records, limits)?),
                "duplicates" => value(a::duplicates::exact(&records, limits)?),
                "findings" => value(
                    records
                        .iter()
                        .map(|r| a::findings::classify(r, limits))
                        .collect::<io::Result<Vec<_>>>()?,
                ),
                "dashboard" => value(
                    a::reporting::build_report(
                        &case,
                        &q,
                        &w::report_options("Case summary / 케이스 요약", "")?,
                        None,
                        None,
                        limits,
                    )?
                    .coverage,
                ),
                _ => Err(invalid("unknown analysis view")),
            }
        }
        _ => Err(invalid("unknown command; use --help")),
    }
}
