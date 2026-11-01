use metaxtract_app::{self as app, core::scan::NativePath};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub authorized_sources: Vec<NativePath>,
    pub recent_inputs: Vec<NativePath>,
    pub favorites: Vec<crate::quick_access::Favorite>,
    pub pending: Vec<crate::recovery::PendingQueue>,
    pub workspace: WorkspacePreferences,
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
            authorized_sources: vec![],
            recent_inputs: vec![],
            favorites: vec![],
            pending: vec![],
            workspace: WorkspacePreferences::default(),
            recent: vec![],
            korean: false,
            language: None,
            appearance: Appearance::System,
            scale: 1.0,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePreferences {
    pub tab: crate::state::Tab,
    pub order: metaxtract_app::storage::RecordOrder,
    pub navigation_width: f32,
    pub inspector_width: f32,
    pub show_inspector: bool,
    pub window_size: [f32; 2],
    pub window_position: Option<[f32; 2]>,
    pub preview_hex: bool,
    pub include_hidden: bool,
    pub decode_previews: bool,
    pub workers: usize,
}
impl Default for WorkspacePreferences {
    fn default() -> Self {
        Self {
            tab: Default::default(),
            order: metaxtract_app::storage::RecordOrder::Path,
            navigation_width: 210.0,
            inspector_width: 410.0,
            show_inspector: true,
            window_size: [1280.0, 820.0],
            window_position: None,
            preview_hex: false,
            include_hidden: false,
            decode_previews: false,
            workers: 2,
        }
    }
}
impl Settings {
    pub fn viewport(&self) -> eframe::egui::ViewportBuilder {
        let w = &self.workspace;
        let size = w.window_size.map(|n| {
            if n.is_finite() {
                n.clamp(620.0, 4096.0)
            } else {
                820.0
            }
        });
        let mut viewport = eframe::egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size([900.0, 620.0]);
        if let Some(position) = w
            .window_position
            .filter(|p| p.iter().all(|n| n.is_finite() && n.abs() < 32768.0))
        {
            viewport = viewport.with_position(position);
        }
        viewport
    }
}

pub fn session_directory() -> io::Result<PathBuf> {
    Ok(path()?.parent().expect("settings parent").join("sessions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_preferences_gain_defaults_without_losing_history() {
        let legacy: Settings = serde_json::from_str(r#"{"recent":[],"korean":true}"#).unwrap();
        assert!(legacy.korean);
        assert_eq!(legacy.scale, 1.0);
        assert!(legacy.language.is_none());
        assert!(legacy.pending.is_empty());
    }
    #[test]
    fn preferences_round_trip_pending_queues_and_workspace_choices() {
        let mut settings = Settings {
            language: Some(crate::i18n::Language::Korean),
            ..Default::default()
        };
        settings.workspace.tab = crate::state::Tab::Analysis;
        settings.workspace.order = app::storage::RecordOrder::Size;
        settings.pending.push(crate::recovery::PendingQueue {
            case: NativePath::from_path(std::path::Path::new("case.mxcase")),
            sources: vec![NativePath::from_path(std::path::Path::new(
                "한글 sample.jpg",
            ))],
            temporary: false,
        });
        let decoded: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(decoded.language, settings.language);
        assert!(decoded.workspace.tab == settings.workspace.tab);
        assert_eq!(decoded.workspace.order, settings.workspace.order);
        assert_eq!(decoded.pending.len(), 1);
        assert!(decoded.pending == settings.pending);
    }
}
