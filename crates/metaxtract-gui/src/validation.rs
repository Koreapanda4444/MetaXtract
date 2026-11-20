//! Explicit opt-in native GUI workflow verification used by the package verifier.
use crate::{
    desktop::{Desktop, Tab},
    workflows::Field,
};
use eframe::egui;
use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};
pub struct Validation {
    directory: PathBuf,
    step: u8,
    started: Instant,
    original: Vec<u8>,
}
impl Validation {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            step: 0,
            started: Instant::now(),
            original: vec![],
        }
    }
}
impl Desktop {
    pub(crate) fn poll_validation(&mut self, ctx: &egui::Context) {
        let Some(mut validation) = self.validation.take() else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(25));
        let outcome = (|| -> io::Result<bool> {
            if validation.started.elapsed() > Duration::from_secs(90) {
                return Err(io::Error::other(format!(
                    "Desktop workflow timed out at step {}: {}",
                    validation.step, self.status
                )));
            }
            if self.busy || self.scan.running() || !self.scan.queue.is_empty() {
                return Ok(false);
            }
            match validation.step {
                0 => {
                    validation.original =
                        std::fs::read(validation.directory.join("validation.png"))?;
                    self.quick_analysis(ctx, validation.directory.join("validation.png"));
                }
                1 => {
                    if self
                        .scan
                        .outcome
                        .as_ref()
                        .is_none_or(|out| out.summary.completed != 1 || out.summary.failed != 0)
                    {
                        return Err(io::Error::other(format!(
                            "Native desktop scan failed: {}",
                            self.status
                        )));
                    }
                    let scan = self
                        .active_scan
                        .as_deref()
                        .ok_or_else(|| io::Error::other("Scan not selected"))?;
                    let query = metaxtract_app::storage::SearchQuery::new(
                        scan,
                        metaxtract_app::storage::SearchExpression::Predicate(
                            metaxtract_app::storage::SearchPredicate {
                                field: metaxtract_app::storage::SearchField::Mime,
                                operator: metaxtract_app::storage::SearchOperator::Equal,
                                value: "image/png".into(),
                            },
                        ),
                    );
                    self.apply_search(query);
                }
                2 => {
                    if self.records.refresh {
                        return Ok(false);
                    }
                    let id = self
                        .records
                        .headers
                        .first()
                        .ok_or_else(|| io::Error::other("Desktop search returned no PNG"))?
                        .id;
                    if self.records.total != 1 {
                        return Err(io::Error::other("Desktop filter count differs"));
                    }
                    self.focus_record(ctx, id);
                }
                3 => {
                    if self.records.focused.is_none() {
                        return Err(io::Error::other(
                            "Inspector did not load the focused record",
                        ));
                    }
                    self.tab = Tab::Analysis;
                    self.request_analysis(ctx);
                }
                4 => {
                    if self
                        .analysis
                        .report
                        .as_ref()
                        .is_none_or(|r| r.records.len() != 1)
                    {
                        return Err(io::Error::other("Desktop analysis snapshot differs"));
                    }
                    self.tab = Tab::Workflows;
                }
                5..=8 => {
                    if validation.step > 5 {
                        let previous = [
                            "validation.csv",
                            "validation.jsonl",
                            "validation.html",
                            "validation.json",
                        ][usize::from(validation.step - 6)];
                        if !validation.directory.join(previous).is_file() {
                            return Err(io::Error::other(format!(
                                "Export missing: {previous}: {}",
                                self.status
                            )));
                        }
                    }
                    let n = usize::from(validation.step - 5);
                    self.workflows.select_action(if n < 2 { 0 } else { 2 });
                    self.workflows.format = if n == 0 || n == 3 { 1 } else { 0 };
                    self.workflows.scope = 1;
                    self.workflow_path(
                        Field::Output,
                        validation.directory.join(
                            [
                                "validation.csv",
                                "validation.jsonl",
                                "validation.html",
                                "validation.json",
                            ][n],
                        ),
                    );
                    self.run_workflow(ctx);
                }
                _ => {
                    if !validation.directory.join("validation.json").is_file() {
                        return Err(io::Error::other(format!(
                            "JSON report missing: {}",
                            self.status
                        )));
                    }
                    if std::fs::read(validation.directory.join("validation.png"))?
                        != validation.original
                    {
                        return Err(io::Error::other(
                            "Desktop workflows changed source evidence",
                        ));
                    }
                    return Ok(true);
                }
            }
            validation.step += 1;
            Ok(false)
        })();
        match outcome {
            Ok(false) => self.validation = Some(validation),
            result => {
                let value = match result {
                    Ok(true) => {
                        serde_json::json!({"ok":true,"open_scan_search_inspect_analyze_export":true,"source_preserved":true})
                    }
                    Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                    _ => unreachable!(),
                };
                if let Err(e) = metaxtract_app::workflows::write_json(
                    &validation.directory.join("desktop-workflows.json"),
                    &value,
                ) {
                    eprintln!("Desktop verification report: {e}");
                }
                self.session_ui.discard_once = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
