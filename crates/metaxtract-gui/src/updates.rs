use crate::desktop::Desktop;
use eframe::egui;
use std::{io, time::Duration};
const RELEASES: &str = "https://github.com/Koreapanda4444/MetaXtract/releases";
#[derive(Default)]
pub struct Updates {
    result: Option<(String, bool)>,
}
fn version(value: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<_, _>>()
        .ok()?;
    parts.try_into().ok()
}
fn check() -> io::Result<(String, bool)> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get("https://api.github.com/repos/Koreapanda4444/MetaXtract/releases/latest")
        .header(
            "User-Agent",
            concat!("MetaXtract/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(io::Error::other)?;
    let body = response
        .body_mut()
        .with_config()
        .limit(262144)
        .read_to_string()
        .map_err(io::Error::other)?;
    let release: serde_json::Value = serde_json::from_str(&body).map_err(io::Error::other)?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| io::Error::other("Release version missing"))?;
    let latest = version(tag).ok_or_else(|| io::Error::other("Unsupported release version"))?;
    if release["draft"] == true || release["prerelease"] == true {
        return Err(io::Error::other("Stable release unavailable"));
    }
    Ok((
        tag.into(),
        latest > version(env!("CARGO_PKG_VERSION")).expect("application version"),
    ))
}
impl Desktop {
    pub fn update_controls(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(format!("MetaXtract {}", env!("CARGO_PKG_VERSION")), |ui| {
            ui.label(crate::i18n::tr("Update checks contact GitHub only when requested. Evidence is never sent."));
            if ui.add_enabled(!self.busy, egui::Button::new(crate::i18n::tr("Check for updates"))).clicked() {
                self.task(ui.ctx(), crate::i18n::tr("Checking for updates"), check, |app, result| app.updates.result = Some(result));
            }
            if let Some((tag,newer))=&self.updates.result {
                ui.label(format!("{}: {tag}", crate::i18n::tr(if *newer {"New version available"} else {"Latest stable version"})));
                ui.hyperlink_to(crate::i18n::tr("Review downloads and checksums"), RELEASES);
            }
            ui.weak(crate::i18n::tr("Download and installation require your action. Back up open cases before upgrading."));
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_numerically_and_reject_prereleases() {
        assert!(version("v0.10.0") > version("0.2.0"));
        assert!(version("v1.0.0-beta").is_none());
        assert!(version("1.2").is_none());
    }
}
