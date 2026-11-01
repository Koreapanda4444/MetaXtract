use metaxtract_app::{self as app, core::scan::NativePath};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub recent: Vec<NativePath>,
    pub korean: bool,
    pub appearance: Appearance,
    #[serde(default = "default_scale")]
    pub scale: f32,
    pub language: Option<crate::i18n::Language>,
}
fn path() -> io::Result<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
    };
    base.map(|p| p.join("MetaXtract/desktop.json"))
        .ok_or_else(|| io::Error::other("OS configuration directory unavailable"))
}
impl Settings {
    pub fn load() -> Self {
        path()
            .ok()
            .and_then(|p| app::workflows::read_json(&p).ok())
            .unwrap_or_default()
    }
    pub fn remember(&mut self, path: &std::path::Path) -> io::Result<()> {
        let path = NativePath::from_path(&path.canonicalize()?);
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(12);
        self.save()
    }
    pub fn save(&self) -> io::Result<()> {
        use std::io::Write;
        let path = path()?;
        let parent = path.parent().expect("settings parent");
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut file, self).map_err(io::Error::other)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}
impl Appearance {
    pub fn apply(self, ctx: &eframe::egui::Context, scale: f32) {
        ctx.set_theme(match self {
            Self::System => eframe::egui::ThemePreference::System,
            Self::Dark => eframe::egui::ThemePreference::Dark,
            Self::Light => eframe::egui::ThemePreference::Light,
        });
        ctx.set_zoom_factor(if scale.is_finite() {
            scale.clamp(0.75, 2.0)
        } else {
            1.0
        });
        crate::components::apply(ctx);
    }
}

fn default_scale() -> f32 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent: vec![],
            korean: false,
            language: None,
            appearance: Appearance::System,
            scale: 1.0,
        }
    }
}
