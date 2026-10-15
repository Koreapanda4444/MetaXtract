use crate::desktop::Desktop;
use eframe::egui;
use metaxtract_app as app;
use std::path::PathBuf;
#[derive(Default)]
pub struct PreviewUi {
    pub record_id: Option<i64>,
    pub content: Option<app::preview::Preview>,
    pub texture: Option<egui::TextureHandle>,
    pub hex: bool,
}
impl Desktop {
    pub fn load_preview(&mut self, ctx: &egui::Context, path: PathBuf) {
        let Some((id, record)) = self.records.focused.clone() else {
            return;
        };
        self.task(
            ctx,
            "Verifying and decoding preview / 원본 확인·미리보기 디코딩",
            move || app::preview::load(&record, &path),
            move |app, content| {
                if app.records.focused.as_ref().map(|r| r.0) != Some(id) {
                    return;
                }
                app.preview.record_id = Some(id);
                app.preview.texture = None;
                app.preview.content = Some(content);
            },
        );
    }
    pub fn preview_view(&mut self, ui: &mut egui::Ui) {
        let Some((id, _)) = &self.records.focused else {
            ui.label("Open a record to preview it. / 미리볼 레코드를 여세요.");
            return;
        };
        if self.preview.record_id != Some(*id) {
            self.preview = Default::default();
            self.preview.record_id = Some(*id);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Choose matching source / 일치하는 원본 선택"),
                )
                .clicked()
            {
                self.picker = Some(crate::files::Picker::new(crate::files::Purpose::Preview));
            }
            ui.checkbox(&mut self.preview.hex, "Hex / 16진수");
            ui.label("64 MiB source · 64 KiB hex · 32 KiB text · first frame / 제한된 미리보기");
        });
        if let Some(content) = &self.preview.content {
            for note in &content.notices {
                ui.small(note);
            }
            if self.preview.texture.is_none() && !content.rgb.is_empty() {
                self.preview.texture = Some(ui.ctx().load_texture(
                    format!("preview-{id}"),
                    egui::ColorImage::from_rgb([content.width, content.height], &content.rgb),
                    egui::TextureOptions::LINEAR,
                ));
            }
            egui::ScrollArea::both().id_salt("record-preview").max_height(260.0).show(ui,|ui|{
                if self.preview.hex{egui::ScrollArea::vertical().id_salt("hex-lines").show_rows(ui,18.0,content.hex.len(),|ui,range|{for line in &content.hex[range]{ui.monospace(line);}});}
                else{
                    if let Some(texture)=&self.preview.texture{ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(content.width as f32,content.height as f32)*(720.0/content.width as f32).min(400.0/content.height as f32).min(1.0)));}
                    if !content.text.is_empty(){ui.add(egui::Label::new(egui::RichText::new(&content.text).monospace()).selectable(true).wrap());}
                    if content.text.is_empty()&&self.preview.texture.is_none(){ui.label("Text/image decoder unavailable. Switch to Hex. / 텍스트·이미지 디코더를 사용할 수 없습니다. 16진수를 선택하세요.");}
                }
            });
        } else {
            ui.label("Select the original or an independently extracted member. Size and SHA-256 must match. / 원본 또는 별도 추출한 멤버를 선택하세요. 크기·SHA-256이 일치해야 합니다.");
        }
    }
}
