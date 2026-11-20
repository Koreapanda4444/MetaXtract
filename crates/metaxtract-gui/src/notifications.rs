use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::Write,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Serialize, Deserialize)]
pub struct Notice {
    id: u64,
    pub title: String,
    pub details: String,
    pub error: bool,
    started_ms: u64,
    duration_ms: Option<u64>,
    state: String,
    #[serde(skip)]
    created: Option<Instant>,
}
pub struct Notifications {
    history: VecDeque<Notice>,
    pub open: bool,
    query: String,
    errors_only: bool,
    next_id: u64,
    persistence_error: Option<String>,
}
fn history_path() -> std::io::Result<std::path::PathBuf> {
    Ok(crate::settings::path()?.with_file_name("history.json"))
}
impl Default for Notifications {
    fn default() -> Self {
        let mut history: VecDeque<Notice> = history_path()
            .ok()
            .and_then(|p| metaxtract_app::read_bytes(&p, 16 * 1024 * 1024).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        history.truncate(512);
        for notice in &mut history {
            if notice.state == "running" {
                notice.state = "interrupted".into();
                notice.error = true;
                notice.details = "Previous session ended before a result was recorded.".into();
            }
        }
        let next_id = history
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Self {
            history,
            open: false,
            query: String::new(),
            errors_only: false,
            next_id,
            persistence_error: None,
        }
    }
}
impl Notifications {
    fn persist(&mut self) {
        let result = (|| -> std::io::Result<()> {
            let path = history_path()?;
            let parent = path.parent().expect("history directory");
            std::fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer(&mut file, &self.history).map_err(std::io::Error::other)?;
            file.flush()?;
            file.as_file().sync_all()?;
            file.persist(path).map_err(|e| e.error)?;
            Ok(())
        })();
        self.persistence_error = result.err().map(|e| e.to_string());
    }
    fn insert(&mut self, title: String, details: String, error: bool, state: &str) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.history.push_front(Notice {
            id,
            title: title.chars().take(256).collect(),
            details: details.chars().take(4096).collect(),
            error,
            started_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            duration_ms: None,
            state: state.into(),
            created: Some(Instant::now()),
        });
        self.history.truncate(512);
        self.persist();
        id
    }
    pub fn push(&mut self, title: impl Into<String>, details: impl Into<String>, error: bool) {
        self.insert(
            title.into(),
            details.into(),
            error,
            if error { "failed" } else { "completed" },
        );
    }
    pub fn start(&mut self, title: &str) -> u64 {
        self.insert(title.into(), String::new(), false, "running")
    }
    pub fn finish(&mut self, id: u64, error: Option<String>, duration: Duration) {
        if let Some(n) = self.history.iter_mut().find(|n| n.id == id) {
            n.error = error.is_some();
            n.details = error.unwrap_or_default().chars().take(4096).collect();
            n.state = if n.error { "failed" } else { "completed" }.into();
            n.duration_ms = Some(duration.as_millis().min(u64::MAX as u128) as u64);
            n.created = Some(Instant::now());
        }
        self.persist();
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("notifications"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -44.0])
            .show(ctx, |ui| {
                for notice in self
                    .history
                    .iter()
                    .filter(|n| {
                        n.created.is_some_and(|created| {
                            created.elapsed() < Duration::from_secs(if n.error { 12 } else { 5 })
                        })
                    })
                    .take(3)
                {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.colored_label(
                            if notice.error {
                                egui::Color32::LIGHT_RED
                            } else {
                                ui.visuals().text_color()
                            },
                            &notice.title,
                        );
                        if ui.small_button(crate::i18n::tr("Details")).clicked() {
                            self.open = true;
                        }
                    });
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            });
        crate::components::modal(crate::i18n::tr("Task and error history")).open(&mut self.open).show(ctx,|ui|{
            ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(crate::i18n::tr("Search tasks and errors")).char_limit(256));ui.checkbox(&mut self.errors_only,crate::i18n::tr("Errors only"));});
            if let Some(e)=&self.persistence_error{ui.colored_label(egui::Color32::YELLOW,format!("History could not be saved: {e}"));}
            let query=self.query.to_lowercase();
            egui::ScrollArea::vertical().id_salt("task-history").max_height(500.0).show(ui,|ui|{for notice in self.history.iter().filter(|n|(!self.errors_only||n.error)&&format!("{} {} {}",n.title,n.details,n.state).to_lowercase().contains(&query)){
                ui.push_id(notice.id,|ui|{ui.collapsing(format!("{} · {} · {}",crate::formatting::unix_seconds(i128::from(notice.started_ms)/1000),notice.state,notice.title),|ui|{if let Some(ms)=notice.duration_ms{ui.label(format!("{ms} ms"));}ui.add(egui::Label::new(&notice.details).selectable(true).wrap());if notice.error{ui.label(crate::i18n::tr("Check the path and access permissions, then retry. Existing results are preserved."));}
if ui.button(crate::i18n::tr("Copy details")).clicked(){ui.ctx().copy_text(format!("{}\n{}\n{}",notice.title,notice.state,notice.details));}});});
            }});
        });
    }
}
