use crate::{desktop::Desktop, formatting::unix_seconds};
use eframe::egui;
use metaxtract_app::analysis::timestamp::parse_timestamp;
#[derive(Default)]
pub struct TimelineUi {
    from: String,
    to: String,
    mime: String,
    zoom: f64,
    pan: f64,
    unresolved: bool,
}
fn bound(text: &str, end: bool) -> Result<Option<i64>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = if text.len() == 10 {
        format!("{text}T{}Z", if end { "23:59:59" } else { "00:00:00" })
    } else {
        text.into()
    };
    parse_timestamp(&value, None)
        .and_then(|t| t.unix_seconds)
        .map(Some)
        .ok_or(())
}
impl Desktop {
    pub(crate) fn timeline_view(&mut self, ui: &mut egui::Ui) {
        let Some(report) = &self.analysis.report else {
            return;
        };
        let state = &mut self.analysis.timeline;
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("UTC from / to"));
            ui.add(
                egui::TextEdit::singleline(&mut state.from)
                    .desired_width(170.0)
                    .hint_text("YYYY-MM-DD")
                    .char_limit(40),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.to)
                    .desired_width(170.0)
                    .hint_text("YYYY-MM-DD")
                    .char_limit(40),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.mime)
                    .desired_width(130.0)
                    .hint_text(crate::i18n::tr("MIME contains"))
                    .char_limit(128),
            );
            ui.checkbox(&mut state.unresolved, "Unresolved only");
        });
        let (Ok(from), Ok(to)) = (bound(&state.from, false), bound(&state.to, true)) else {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "Enter a valid date or ISO timestamp with a timezone.",
            );
            return;
        };
        if from.zip(to).is_some_and(|(a, b)| a > b) {
            ui.label(crate::i18n::tr("Start must precede end."));
            return;
        }
        let types: std::collections::BTreeMap<_, _> = report
            .records
            .iter()
            .map(|r| {
                (
                    r.path.to_string(),
                    r.metadata
                        .pointer("/source/detected_media_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            })
            .collect();
        let indices: Vec<_> = report
            .timeline
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let resolved = e.valid && e.timestamp.unix_seconds.is_some();
                (!state.unresolved || !resolved)
                    && (state.mime.is_empty()
                        || types
                            .get(&e.path.to_string())
                            .is_some_and(|m| m.contains(&state.mime)))
                    && match e.timestamp.unix_seconds.filter(|_| e.valid) {
                        Some(t) => from.is_none_or(|f| t >= f) && to.is_none_or(|v| t <= v),
                        None => from.is_none() && to.is_none(),
                    }
            })
            .map(|(i, _)| i)
            .collect();
        state.zoom = state.zoom.clamp(1.0, 1000.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut state.zoom, 1.0..=1000.0)
                    .logarithmic(true)
                    .text(crate::i18n::tr("Zoom")),
            );
            if ui.button(crate::i18n::tr("Fit timeline")).clicked() {
                state.zoom = 1.0;
                state.pan = 0.0;
            }
        });
        let times: Vec<_> = indices
            .iter()
            .filter_map(|i| {
                report.timeline[*i]
                    .timestamp
                    .unix_seconds
                    .filter(|_| report.timeline[*i].valid)
            })
            .collect();
        if let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) {
            let span = (*last as f64 - *first as f64).max(1.0) / state.zoom;
            let (r, p) = ui.allocate_painter(
                egui::vec2(ui.available_width().max(100.0), 165.0),
                egui::Sense::click_and_drag(),
            );
            if r.dragged() {
                state.pan -=
                    ui.input(|i| i.pointer.delta().x) as f64 / r.rect.width() as f64 * span;
            }
            let middle = (*first as f64 + *last as f64) / 2.0 + state.pan;
            let start = middle - span / 2.0;
            let rect = r.rect.shrink2(egui::vec2(15.0, 28.0));
            p.rect_filled(r.rect, 4.0, ui.visuals().faint_bg_color);
            for n in 0..=4 {
                let x = rect.left() + rect.width() * n as f32 / 4.0;
                let t = start + span * n as f64 / 4.0;
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(0.5, egui::Color32::GRAY),
                );
                p.text(
                    egui::pos2(x, r.rect.bottom() - 10.0),
                    if n == 0 {
                        egui::Align2::LEFT_BOTTOM
                    } else if n == 4 {
                        egui::Align2::RIGHT_BOTTOM
                    } else {
                        egui::Align2::CENTER_BOTTOM
                    },
                    unix_seconds(t as i128).replacen(' ', "\n", 1),
                    egui::FontId::monospace(10.0),
                    ui.visuals().text_color(),
                );
            }
            for (lane, i) in indices.iter().take(5000).enumerate() {
                let e = &report.timeline[*i];
                if let Some(t) = e.timestamp.unix_seconds.filter(|_| e.valid) {
                    let point = egui::pos2(
                        rect.left() + ((t as f64 - start) / span) as f32 * rect.width(),
                        rect.top() + 12.0 + (lane % 5) as f32 * 16.0,
                    );
                    if rect.contains(point) {
                        p.circle_filled(
                            point,
                            if self.analysis.detail == *i { 6.0 } else { 3.5 },
                            egui::Color32::LIGHT_BLUE,
                        );
                        if r.clicked()
                            && r.interact_pointer_pos()
                                .is_some_and(|v| v.distance(point) < 8.0)
                        {
                            self.analysis.detail = *i;
                        }
                    }
                }
            }
        }
        ui.label(format!("{} events · {} unresolved; plotted at most 5,000. Drag to pan. Unknown zones stay unresolved.",indices.len(),indices.iter().filter(|i|report.timeline[**i].timestamp.unix_seconds.is_none()).count()));
        egui::ScrollArea::vertical()
            .id_salt("timeline-events")
            .max_height(220.0)
            .show_rows(ui, 30.0, indices.len(), |ui, range| {
                for n in range {
                    let i = indices[n];
                    let e = &report.timeline[i];
                    let time = e
                        .timestamp
                        .unix_seconds
                        .filter(|_| e.valid)
                        .map(|t| unix_seconds(t.into()))
                        .unwrap_or_else(|| format!("Unresolved: {}", e.timestamp.original));
                    if ui
                        .selectable_label(
                            self.analysis.detail == i,
                            format!("{time} · {} · {}", e.event, e.path),
                        )
                        .clicked()
                    {
                        self.analysis.detail = i;
                    }
                }
            });
        let mut open = None;
        if let Some(e) = report
            .timeline
            .get(self.analysis.detail)
            .filter(|_| indices.contains(&self.analysis.detail))
        {
            if ui
                .add_enabled(!self.busy, egui::Button::new(crate::i18n::tr("Open file")))
                .clicked()
            {
                open = Some((report.scan_id.clone(), e.path.clone()));
            }
            crate::inspectors::json_view(ui, e);
        }
        if let Some((scan, path)) = open {
            self.navigate_to_path(ui.ctx(), scan, path);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_require_valid_resolved_dates() {
        assert!(bound("2024-02-30", false).is_err());
        assert!(bound("2024-02-29", false).unwrap().is_some());
        assert!(bound("2024-01-01T00:00:00", false).is_err());
        assert!(bound("", false).unwrap().is_none());
    }
}
