#![forbid(unsafe_code)]
mod args;
mod output;
mod workflows;
use args::Args;
use metaxtract_app::{self as app, analysis, core::scan::ScanControl, storage};
use serde_json::{Value, json};
use std::io;

const HELP: &str = "MetaXtract Rust CLI\n  scan ROOT --case CASE [--workers N] [--journal PATH] [--resume RUN --scan ID]\n  inspect FILE\n  compare CASE OLD_SCAN NEW_SCAN\n  verify CASE\n  case create CASE [TITLE]\n  case info|history CASE\n  case notes CASE TEXT\n  search CASE SCAN [--query TEXT --limit N --cursor JSON --save NAME]\n  export|redact CASE SCAN OUTPUT [--format csv|jsonl --profile JSON]\n  redact CASE SCAN --preview RECORD_ID\n  report CASE SCAN OUTPUT [--format html|json --title TEXT --analyst NAME]\n  analyze CASE SCAN [--view dashboard|timeline|locations|duplicates|findings]\n  import CASE INPUT [--kind jsonl|python|python-bundle]\n  keygen PRIVATE_PEM PUBLIC_PEM --name NAME --password-stdin\n  sign INPUT SIGNATURE --key PRIVATE_PEM --password-stdin\n  verify-signature INPUT SIGNATURE PUBLIC_PEM\n  bundle export CASE OUTPUT [--key PRIVATE_PEM --password-stdin]\n  bundle verify INPUT [--public-key PEM]\n  bundle open INPUT CASE [--public-key PEM]\n\nBuild the bundled metaxtract-worker beside this executable. / 전용 파서 실행 파일을 함께 빌드하세요.";
fn run(a: &Args) -> io::Result<Value> {
    match a.word(0)?.as_str() {
        "scan" => {
            let case = a.flag_path("case").ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "scan requires --case PATH")
            })?;
            let mut request = app::scan::ScanRequest::new(a.path(1)?, case);
            if let Some(p) = a.flag_path("journal") {
                request.journal_path = p;
            }
            if let Some(n) = a.flag("workers") {
                request.workers = n.parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid worker count")
                })?;
            }
            if let Some(n) = a.flag("resume") {
                request.resume = Some((
                    n.parse()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid run"))?,
                    a.flag("scan").ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "resume requires --scan ID")
                    })?,
                ));
            }
            let control = ScanControl::new();
            let pause_after = a
                .flag("pause-after")
                .map(|s| {
                    s.parse::<u64>().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid pause count")
                    })
                })
                .transpose()?;
            if pause_after == Some(0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "pause count must be positive",
                ));
            }
            let result = app::scan::run(&request, control.clone(), |event| {
                if pause_after.is_some_and(|n| event.summary.completed >= n) {
                    control.pause();
                }
                if a.flags.contains_key("progress") {
                    eprintln!(
                        "{}",
                        serde_json::to_string(&json!({"event":"scan","data":event}))
                            .map_err(io::Error::other)?
                    );
                }
                Ok(())
            })?;
            serde_json::to_value(result).map_err(io::Error::other)
        }
        "inspect" => {
            let temp = tempfile::tempdir()?;
            let path = temp.path().join("inspect.mxcase");
            let result = app::scan::run(
                &app::scan::ScanRequest::new(a.path(1)?, path.clone()),
                ScanControl::new(),
                |_| Ok(()),
            )?;
            let case = app::open_case(&path, false)?;
            let q = analysis::search::parse(&result.scan_id, "")?;
            Ok(json!({"summary":result,"records":app::collect(&case,&q)?}))
        }
        "verify" => {
            serde_json::to_value(app::open_case(&a.path(1)?, false)?.verify_integrity(None)?)
                .map_err(io::Error::other)
        }
        "compare" => {
            let case = app::open_case(&a.path(1)?, false)?;
            let old = app::collect(&case, &analysis::search::parse(&a.word(2)?, "")?)?;
            let new = app::collect(&case, &analysis::search::parse(&a.word(3)?, "")?)?;
            serde_json::to_value(analysis::diff::compare_scans(
                &old,
                &new,
                Default::default(),
            )?)
            .map_err(io::Error::other)
        }
        "case" => match a.word(1)?.as_str() {
            "create" => serde_json::to_value(
                storage::CaseWorkspace::create(
                    a.path(2)?,
                    &a.word(3).unwrap_or_else(|_| "MetaXtract case".into()),
                )?
                .info()?,
            )
            .map_err(io::Error::other),
            "info" => serde_json::to_value(app::open_case(&a.path(2)?, false)?.info()?)
                .map_err(io::Error::other),
            "history" => {
                let cursor = a
                    .flag("cursor")
                    .map(|c| {
                        serde_json::from_str::<storage::ScanCursor>(&c).map_err(io::Error::other)
                    })
                    .transpose()?;
                let limit = a
                    .flag("limit")
                    .map(|c| {
                        c.parse::<usize>().map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidInput, "invalid page limit")
                        })
                    })
                    .transpose()?
                    .unwrap_or(128);
                let page =
                    app::open_case(&a.path(2)?, false)?.scan_history(cursor.as_ref(), limit)?;
                Ok(json!({"items":page.items,"next":page.next}))
            }
            "notes" => {
                let case = app::open_case(&a.path(2)?, true)?;
                case.set_notes(&a.word(3)?)?;
                Ok(json!({"saved":true}))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown case command",
            )),
        },
        _ => workflows::run(a),
    }
}
fn main() {
    let mut command = None;
    let result = Args::parse(std::env::args_os().skip(1)).and_then(|a| {
        if a.flags.contains_key("help") || a.words.is_empty() {
            println!("{HELP}");
            return Ok(0);
        }
        command = Some(a.word(0)?);
        a.validate()?;
        let value = run(&a)?;
        let code = output::result_code(&value);
        output::write(
            &json!({"ok":true,"command":command,"result":value}),
            a.flags.contains_key("pretty"),
        )?;
        Ok(code)
    });
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
        Err(error) => {
            let code = output::error_code(&error);
            eprintln!(
                "{}",
                json!({"ok":false,"command":command,"error":{"kind":output::error_kind(&error),"message":error.to_string(),"exit_code":code}})
            );
            std::process::exit(code);
        }
    }
}
