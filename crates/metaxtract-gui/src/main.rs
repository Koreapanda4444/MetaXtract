#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]
mod analysis;
mod application;
mod components;
mod desktop;
mod files;
mod i18n;
mod ingestion;
mod inspectors;
mod notifications;
mod preview;
mod quick_access;
mod records;
mod recovery;
mod scan;
mod search;
mod sessions;
mod settings;
mod state;
mod tasks;
mod workflows;
mod workspace;
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
    let mut analyze = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--smoke" {
            smoke = true;
        } else if arg == "--analyze" {
            analyze = args.next().map(PathBuf::from);
            if analyze.is_none() {
                eprintln!("--analyze requires a file or directory");
                std::process::exit(2);
            }
        } else if arg == "--help" {
            println!(
                "MetaXtract desktop / 데스크톱\n  metaxtract-gui [CASE | --analyze FILE_OR_DIRECTORY] [--smoke]\nA bundled metaxtract-worker is required for scans. / 스캔에는 전용 파서 실행 파일이 필요합니다."
            );
            return Ok(());
        } else if open.is_none() {
            open = Some(PathBuf::from(arg));
        } else {
            eprintln!("unexpected argument / 잘못된 인수");
            std::process::exit(2);
        }
    }
    if open.is_some() && analyze.is_some() {
        eprintln!("Choose a case or a quick-analysis source");
        std::process::exit(2);
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("bundled icon");
    let options = eframe::NativeOptions {
        viewport: settings::Settings::load().viewport().with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "MetaXtract",
        options,
        Box::new(move |cc| {
            let korean = fonts(&cc.egui_ctx);
            let mut desktop = desktop::Desktop::new(cc, open, smoke, korean);
            if let Some(path) = analyze {
                desktop.quick_analysis(&cc.egui_ctx, path);
            }
            Ok(Box::new(desktop))
        }),
    )
}
