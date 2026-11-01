use crate::desktop::{Desktop, Tab};
use eframe::egui;
use metaxtract_app::storage::{
    SearchExpression as E, SearchField as F, SearchOperator as O, SearchPredicate, SearchQuery,
};
use std::time::{Duration, Instant};
impl Desktop {
    pub(crate) fn explorer_search(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.search.text)
                        .desired_width((ui.available_width() - 110.0).max(100.0))
                        .hint_text(crate::i18n::tr("Search files and metadata"))
                        .char_limit(4096),
                )
                .changed();
            if ui.button(crate::i18n::tr("Reset filters")).clicked() {
                self.search.text.clear();
                self.search.visual = Default::default();
                self.search.advanced = false;
                self.records.mime_filter.clear();
                self.records.status_filter = None;
                if let Some(scan) = &self.active_scan {
                    self.apply_search(SearchQuery::new(scan, E::All(vec![])));
                }
                self.search.pending = None;
            }
        });
        ui.collapsing(crate::i18n::tr("Conditions · AND / OR / NOT"), |ui| {
            changed |= self.search.visual.ui(ui);
        });
        if self.search.advanced {
            ui.label("Advanced JSON is active; edit it in Search & compare.");
        }
        if changed {
            self.search.pending = Some(Instant::now());
        }
        if let Some(since) = self.search.pending {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            if since.elapsed() >= Duration::from_millis(300) && !self.busy {
                self.search.pending = None;
                match self.search_query() {
                    Ok(mut q) => {
                        q.records.mime = (!self.records.mime_filter.trim().is_empty())
                            .then(|| self.records.mime_filter.trim().into());
                        q.records.status = self.records.status_filter;
                        self.apply_search(q);
                    }
                    Err(e) => self.report_error(e),
                }
            }
        }
    }
    pub(crate) fn navigate_to_path(
        &mut self,
        ctx: &egui::Context,
        scan: String,
        path: impl ToString,
    ) {
        let Some(case) = self.case.clone() else {
            return;
        };
        let q = SearchQuery::new(
            &scan,
            E::Predicate(SearchPredicate {
                field: F::Path,
                operator: O::Equal,
                value: path.to_string(),
            }),
        );
        let context = ctx.clone();
        self.task(
            ctx,
            "Opening linked file",
            move || {
                let page = case.search_records(&q, None, 1)?;
                let id = page
                    .items
                    .first()
                    .map(|r| r.id)
                    .ok_or_else(|| std::io::Error::other("File is absent from this scan"))?;
                Ok((q, id))
            },
            move |app, (q, id)| {
                app.search.visual.load(q.clone());
                app.search.text.clear();
                app.search.advanced = false;
                app.apply_search(q);
                app.tab = Tab::Records;
                app.focus_record(&context, id);
            },
        );
    }
}
