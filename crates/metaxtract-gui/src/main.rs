#![forbid(unsafe_code)]
mod desktop;
mod files;
mod preview;
mod records;
mod scan;
mod settings;
use eframe::egui;
use std::path::PathBuf;

fn fonts(ctx: &egui::Context) -> bool {
    let candidates = std::env::var_os("METAXTRACT_FONT_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            PathBuf::from("C:/Windows/Fonts/malgun.ttf"),
            PathBuf::from("/System/Library/Fonts/AppleSDGothicNeo.ttc"),
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/nanum/NanumGothic.ttf"),
        ]);
    for path in candidates {
        if let Ok(bytes) = metaxtract_app::read_bytes(&path, 32 * 1024 * 1024) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("korean".into(), egui::FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("korean".into());
            }
            ctx.set_fonts(fonts);
            return true;
        }
    }
    false
}
fn main() -> eframe::Result {
    let mut open = None;
    let mut smoke = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--smoke" {
            smoke = true;
        } else if arg == "--help" {
            println!(
                "MetaXtract desktop / 데스크톱\n  metaxtract-gui [CASE] [--smoke]\nA bundled metaxtract-worker is required for scans. / 스캔에는 전용 파서 실행 파일이 필요합니다."
            );
            return Ok(());
        } else if open.is_none() {
            open = Some(PathBuf::from(arg));
        } else {
            eprintln!("unexpected argument / 잘못된 인수");
            std::process::exit(2);
        }
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("bundled icon");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 620.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "MetaXtract",
        options,
        Box::new(move |cc| {
            let korean = fonts(&cc.egui_ctx);
            Ok(Box::new(desktop::Desktop::new(cc, open, smoke, korean)))
        }),
    )
}
