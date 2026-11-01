use crate::{desktop::Desktop, inspectors::claim_brief};
use eframe::egui;
use metaxtract_app::{
    self as app,
    schema::EvidencePath,
    storage::{
        SearchExpression as E, SearchField as F, SearchOperator as O, SearchPredicate, SearchQuery,
    },
};
#[derive(Default)]
pub struct PairUi {
    paths: Option<(EvidencePath, EvidencePath)>,
    previews: Vec<Result<app::preview::Preview, String>>,
    textures: Vec<Option<egui::TextureHandle>>,
    minimum: u16,
}
impl Desktop {
    fn load_pair(
        &mut self,
        ctx: &egui::Context,
        scan: String,
        paths: (EvidencePath, EvidencePath),
    ) {
        let Some(case) = self.case.clone() else {
            return;
        };
        let roots = self.settings.authorized_sources.clone();
        self.analysis.pair.paths = Some(paths.clone());
        self.analysis.pair.previews.clear();
        self.analysis.pair.textures.clear();
        self.task(ctx,"Loading verified comparison previews",move || {
            let revision=case.info()?.revision;let mut previews=vec![];
            for path in [&paths.0,&paths.1] {
                let load=||->std::io::Result<_>{
                    let q=SearchQuery::new(&scan,E::Predicate(SearchPredicate{field:F::Path,operator:O::Equal,value:path.to_string()}));
                    let page=case.search_records(&q,None,1)?;let id=page.items.first().ok_or_else(||std::io::Error::other("Missing file"))?.id;
                    let r=case.record(id)?.ok_or_else(||std::io::Error::other("Missing record"))?;
                    let source=app::preview::source_candidate(&case,&scan,&r)?.canonicalize()?;
                    if !crate::file_actions::authorized(&source,&roots){return Err(std::io::Error::other("Choose the matching original in the file inspector to authorize its preview"));}
                    app::preview::load(&r,&source)
                };
                previews.push(load().map_err(|e|e.to_string()));
            }
            if case.info()?.revision!=revision {return Err(std::io::Error::other("Case changed during comparison; retry"));}Ok((paths,previews))
        },|s,(paths,previews)|{if s.analysis.pair.paths.as_ref()==Some(&paths){s.analysis.pair.previews=previews;s.analysis.pair.textures=vec![None,None];}});
    }
    pub(crate) fn duplicate_review(&mut self, ui: &mut egui::Ui) {
        let Some(report) = &self.analysis.report else {
            return;
        };
        let mut pair = None;
        let mut open = None;
        ui.label(crate::i18n::tr("Exact groups share a recorded cryptographic digest and size. Similarity scores are heuristic."));
        ui.add(
            egui::Slider::new(&mut self.analysis.pair.minimum, 0..=1000)
                .text(crate::i18n::tr("Minimum similarity (‰)")),
        );
        egui::ScrollArea::vertical()
            .id_salt("duplicate-groups")
            .max_height(240.0)
            .show(ui, |ui| {
                for (i, g) in report.exact_duplicates.iter().enumerate() {
                    ui.collapsing(
                        format!(
                            "Exact · {:?} · {} files · {}",
                            g.algorithm,
                            g.paths.len(),
                            g.digest
                        ),
                        |ui| {
                            for path in &g.paths {
                                ui.horizontal(|ui| {
                                    if ui.small_button(crate::i18n::tr("Open")).clicked() {
                                        open = Some((report.scan_id.clone(), path.clone()));
                                    }
                                    ui.label(path.to_string());
                                    if path != &g.paths[0]
                                        && ui
                                            .add_enabled(
                                                !self.busy,
                                                egui::Button::new(crate::i18n::tr(
                                                    "Compare with first",
                                                )),
                                            )
                                            .clicked()
                                    {
                                        pair = Some((g.paths[0].clone(), path.clone()));
                                    }
                                });
                            }
                        },
                    )
                    .header_response
                    .on_hover_text(format!(
                        "Group {} · {} bytes",
                        i + 1,
                        g.size_bytes
                    ));
                }
                for (label, matches) in [
                    ("Visual", &report.visually_similar),
                    ("Content", &report.content_similar),
                ] {
                    ui.collapsing(
                        format!("{label} similarity · {} pairs", matches.len()),
                        |ui| {
                            for m in matches
                                .iter()
                                .filter(|m| m.score_millis >= self.analysis.pair.minimum)
                                .take(500)
                            {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!(
                                        "{}‰ · {} ↔ {} · {:?} · {}",
                                        m.score_millis,
                                        m.first,
                                        m.second,
                                        m.method,
                                        if m.heuristic {
                                            "heuristic"
                                        } else {
                                            "measured match"
                                        }
                                    ));
                                    if ui
                                        .add_enabled(
                                            !self.busy,
                                            egui::Button::new(crate::i18n::tr("Compare pair")),
                                        )
                                        .clicked()
                                    {
                                        pair = Some((m.first.clone(), m.second.clone()));
                                    }
                                });
                            }
                            ui.weak(crate::i18n::tr(
                                "Showing up to 500 qualifying pairs per method.",
                            ));
                        },
                    );
                }
            });
        ui.label(format!(
            "Fingerprints: {} · unavailable: {} · content: {}",
            report.coverage.visual_records_with_previews,
            report.coverage.visual_unavailable.len(),
            report.coverage.content_similarity_status
        ));
        ui.collapsing(crate::i18n::tr("Unavailable preview coverage"), |ui| {
            crate::inspectors::json_view(ui, &report.coverage.visual_unavailable)
        });
        if let Some(paths) = &self.analysis.pair.paths {
            ui.separator();
            ui.columns(2, |cols| {
                for (i, path) in [&paths.0, &paths.1].iter().enumerate() {
                    let ui = &mut cols[i];
                    ui.strong(path.to_string());
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(crate::i18n::tr("Open file inspector")),
                        )
                        .clicked()
                    {
                        open = Some((report.scan_id.clone(), (*path).clone()));
                    }
                    if let Some(result) = self.analysis.pair.previews.get(i) {
                        match result {
                            Ok(p) => {
                                ui.label(if p.verified {
                                    "Size and SHA-256 verified"
                                } else {
                                    "Unverified"
                                });
                                if !p.rgb.is_empty() {
                                    if self.analysis.pair.textures[i].is_none() {
                                        let image =
                                            egui::ColorImage::from_rgb([p.width, p.height], &p.rgb);
                                        self.analysis.pair.textures[i] =
                                            Some(ui.ctx().load_texture(
                                                format!("pair-{i}"),
                                                image,
                                                Default::default(),
                                            ));
                                    }
                                    if let Some(t) = &self.analysis.pair.textures[i] {
                                        ui.image((
                                            t.id(),
                                            t.size_vec2()
                                                * (200.0 / t.size_vec2().x.max(1.0)).min(1.0),
                                        ));
                                    }
                                } else {
                                    ui.add(
                                        egui::Label::new(
                                            p.text.chars().take(500).collect::<String>(),
                                        )
                                        .wrap(),
                                    );
                                }
                                for notice in &p.notices {
                                    ui.weak(crate::i18n::bilingual(notice));
                                }
                            }
                            Err(e) => {
                                ui.colored_label(egui::Color32::YELLOW, e);
                            }
                        }
                    } else {
                        ui.spinner();
                    }
                    if let Some(r) = report.records.iter().find(|r| r.path == **path) {
                        for claim in r.original_claims.iter().take(24) {
                            ui.label(format!("{}: {}", claim.key, claim_brief(&claim.value)));
                        }
                    }
                }
            });
        }
        let scan = report.scan_id.clone();
        if let Some(paths) = pair {
            self.load_pair(ui.ctx(), scan, paths);
        } else if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
