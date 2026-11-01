use crate::{
    desktop::{Desktop, Tab},
    files::{Picker, Purpose},
};
use eframe::egui;
impl Desktop {
    pub fn explorer_view(&mut self, ui: &mut egui::Ui) {
        ui.strong(crate::i18n::tr("Files"));
        if ui
            .add_enabled(
                !self.busy && !self.readonly,
                crate::components::button(crate::i18n::tr("Add files or folder")),
            )
            .clicked()
        {
            self.picker = Some(Picker::new(Purpose::Source));
        }
        if let Some(info) = &self.info {
            ui.label(&info.title);
        }
        let mut scan = None;
        egui::ScrollArea::vertical()
            .id_salt("explorer-scans")
            .max_height(220.0)
            .show(ui, |ui| {
                for entry in &self.scans {
                    if ui
                        .selectable_label(
                            self.active_scan.as_ref() == Some(&entry.id),
                            format!("{} · {}", entry.record_count, entry.source),
                        )
                        .on_hover_text(&entry.id)
                        .clicked()
                    {
                        scan = Some(entry.id.clone());
                    }
                }
            });
        if let Some(scan) = scan {
            self.active_scan = Some(scan);
            self.tab = Tab::Records;
        }
        ui.separator();
        ui.label(crate::textf!(
            "{} waiting sources",
            "대기 중인 원본 {}개",
            self.scan.queue.len()
        ));
        self.quick_access_view(ui);
    }
    pub fn workspace_view(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::right("file-inspector")
            .default_width(380.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inspector-panel")
                    .show(ui, |ui| self.record_detail_view(ui));
            });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("file-workspace")
                .show(ui, |ui| self.records_view(ui));
        });
    }
}
