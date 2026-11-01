use crate::tasks::Callback;
use crate::{files::Picker, settings::Settings};
use metaxtract_app::storage::{self, CaseInfo, CaseWorkspace, ScanInfo};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Tab {
    #[default]
    Case,
    Scan,
    Records,
    Search,
    Analysis,
    Workflows,
}
pub struct Desktop {
    pub tab: Tab,
    pub scan: crate::scan::ScanUi,
    pub records: crate::records::RecordsUi,
    pub preview: crate::preview::PreviewUi,
    pub inspect: crate::inspectors::InspectUi,
    pub search: crate::search::SearchUi,
    pub analysis: crate::analysis::AnalysisUi,
    pub workflows: crate::workflows::WorkflowsUi,
    pub temporary_session: Option<tempfile::TempDir>,
    pub case: Option<Arc<CaseWorkspace>>,
    pub info: Option<CaseInfo>,
    pub scans: Vec<ScanInfo>,
    pub history_next: Option<storage::ScanCursor>,
    pub active_scan: Option<String>,
    pub notes: String,
    pub title: String,
    pub picker: Option<Picker>,
    pub settings: Settings,
    pub(crate) settings_checkpoint: std::time::Instant,
    pub(crate) saved_settings: String,
    pub status: String,
    pub busy: bool,
    pub notifications: crate::notifications::Notifications,
    pub readonly: bool,
    pub(crate) sender: mpsc::SyncSender<Callback>,
    pub(crate) receiver: mpsc::Receiver<Callback>,
    pub(crate) smoke: bool,
    pub(crate) frames: u32,
}
