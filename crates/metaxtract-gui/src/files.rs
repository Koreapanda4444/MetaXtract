//! Bounded native-path picker, without a subprocess or web dependency.
use eframe::egui;
use std::{io, path::PathBuf};
#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    Open,
    New,
    Copy,
    Source,
    Csv,
    Jsonl,
    Preview,
    GeoJson,
    Kml,
}
pub struct Picker {
    pub purpose: Purpose,
    current: PathBuf,
    location: String,
    name: String,
    selected: Option<PathBuf>,
    entries: Vec<(PathBuf, bool)>,
    error: String,
}
impl Picker {
    pub fn new(purpose: Purpose) -> Self {
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut p = Self {
            purpose,
            current,
            location: String::new(),
            name: String::new(),
            selected: None,
            entries: vec![],
            error: String::new(),
        };
        p.reload();
        p
    }
    fn reload(&mut self) {
        self.location = self.current.to_string_lossy().into_owned();
        self.entries.clear();
        self.selected = None;
        let result = (|| -> io::Result<()> {
            for entry in std::fs::read_dir(&self.current)?.take(1001) {
                let entry = entry?;
                let kind = entry.file_type()?;
                if !kind.is_symlink() {
                    self.entries.push((entry.path(), kind.is_dir()));
                }
            }
            self.entries
                .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            if self.entries.len() > 1000 {
                self.entries.truncate(1000);
                self.error="Showing the first 1,000 entries. Enter a path to reach other items. / 처음 1,000개만 표시합니다. 다른 항목은 경로를 입력하세요.".into();
            } else {
                self.error.clear();
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.error = e.to_string();
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) -> (bool, Option<PathBuf>) {
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Choose path / 경로 선택")
            .id(egui::Id::new("path-picker"))
            .default_size([720.0, 490.0])
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Parent / 상위").clicked() {
                        if let Some(p) = self.current.parent() {
                            self.current = p.to_owned();
                            self.reload();
                        }
                    }
                    ui.text_edit_singleline(&mut self.location);
                    if ui.button("Go / 이동").clicked() {
                        self.current = PathBuf::from(&self.location);
                        self.reload();
                    }
                });
                let mut navigate = None;
                egui::ScrollArea::vertical()
                    .max_height(330.0)
                    .show(ui, |ui| {
                        for (path, is_dir) in &self.entries {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            let row = ui.selectable_label(
                                self.selected.as_ref() == Some(path),
                                format!("{} {name}", if *is_dir { "[DIR]" } else { "     " }),
                            );
                            if row.clicked() {
                                self.selected = Some(path.clone());
                                self.name = name.into_owned();
                            }
                            if row.double_clicked() && *is_dir {
                                navigate = Some(path.clone());
                            }
                        }
                    });
                if let Some(path) = navigate {
                    self.current = path;
                    self.name.clear();
                    self.reload();
                }
                ui.label("Name or full path / 이름 또는 전체 경로");
                if ui.text_edit_singleline(&mut self.name).changed() {
                    self.selected = None;
                }
                ui.colored_label(egui::Color32::LIGHT_RED, &self.error);
                if matches!(self.purpose, Purpose::Source)
                    && ui
                        .button("Select this directory / 현재 폴더 선택")
                        .clicked()
                {
                    chosen = Some(self.current.clone());
                }
                if ui.button("Choose / 선택").clicked() {
                    let path = self
                        .selected
                        .clone()
                        .unwrap_or_else(|| self.current.join(&self.name));
                    if path.is_dir() {
                        self.current = path;
                        self.name.clear();
                        self.reload();
                    } else if !self.name.is_empty() {
                        chosen = Some(path);
                    }
                }
            });
        (open, chosen)
    }
}
