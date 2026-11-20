use crate::{
    desktop::{Desktop, Tab},
    files::{Picker, Purpose},
};
use eframe::egui;
#[derive(Default)]
pub struct CommandUi {
    pub open: bool,
    pub query: String,
    pub focus_search: bool,
    was_open: bool,
}
#[derive(Clone, Copy)]
enum Command {
    Open,
    Quick,
    New,
    Files,
    Find,
    Analyze,
    Export,
    Compare,
    Queue,
    Details,
    Theme,
    History,
}
const COMMANDS: [(Command, &str, &str); 12] = [
    (Command::Open, "Open case", "Ctrl/Cmd+O"),
    (Command::Quick, "Quick analysis", "Ctrl/Cmd+Shift+O"),
    (Command::New, "New case", "Ctrl/Cmd+N"),
    (Command::Files, "Files", ""),
    (Command::Find, "Search files and metadata", "Ctrl/Cmd+F"),
    (Command::Analyze, "Analysis", "Ctrl/Cmd+Shift+A"),
    (Command::Export, "Export records", "Ctrl/Cmd+E"),
    (Command::Compare, "Compare scans", ""),
    (Command::Queue, "Scan queue", ""),
    (Command::Details, "Show file details", ""),
    (Command::Theme, "Toggle theme", ""),
    (Command::History, "Notifications", ""),
];
impl Desktop {
    fn execute_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Open | Command::Quick | Command::New if !self.busy && !self.scan.running() => {
                self.picker = Some(Picker::new(match command {
                    Command::Open => Purpose::Open,
                    Command::Quick => Purpose::Quick,
                    _ => Purpose::New,
                }))
            }
            Command::Open | Command::Quick | Command::New => {}
            Command::Files => self.tab = Tab::Records,
            Command::Find => {
                self.tab = Tab::Records;
                self.commands.focus_search = true;
            }
            Command::Analyze => self.tab = Tab::Analysis,
            Command::Export => {
                self.tab = Tab::Workflows;
                self.workflows.select_action(0);
            }
            Command::Compare => {
                self.tab = Tab::Search;
                self.search.compare = true;
            }
            Command::Queue => self.tab = Tab::Scan,
            Command::Details => {
                self.settings.workspace.show_inspector = !self.settings.workspace.show_inspector
            }
            Command::Theme => {
                self.settings.appearance = match self.settings.appearance {
                    crate::settings::Appearance::Dark => crate::settings::Appearance::Light,
                    _ => crate::settings::Appearance::Dark,
                };
                self.settings.appearance.apply(ctx, self.settings.scale);
            }
            Command::History => self.notifications.open = true,
        }
    }
    pub(crate) fn command_shortcuts(&mut self, ctx: &egui::Context) {
        let selected = ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)
                || i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::P,
                )
            {
                self.commands.open = true;
                self.commands.query.clear();
            }
            for (modifiers, key, command) in [
                (
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::O,
                    Command::Quick,
                ),
                (
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::A,
                    Command::Analyze,
                ),
                (egui::Modifiers::COMMAND, egui::Key::O, Command::Open),
                (egui::Modifiers::COMMAND, egui::Key::N, Command::New),
                (egui::Modifiers::COMMAND, egui::Key::F, Command::Find),
                (egui::Modifiers::COMMAND, egui::Key::E, Command::Export),
            ] {
                if i.consume_key(modifiers, key) {
                    return Some(command);
                }
            }
            None
        });
        if let Some(command) = selected {
            self.execute_command(ctx, command);
        }
    }
    pub(crate) fn command_palette(&mut self, ctx: &egui::Context) {
        let mut open = self.commands.open;
        let focus = open && !self.commands.was_open;
        let mut selected = None;
        egui::Window::new(crate::i18n::tr("Command search"))
            .id(egui::Id::new("command-palette"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .default_width(460.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.commands.query)
                        .hint_text(crate::i18n::tr("Search commands"))
                        .char_limit(128)
                        .desired_width(f32::INFINITY),
                );
                if focus {
                    response.request_focus();
                }
                let query = self.commands.query.to_lowercase();
                let mut first = true;
                for (command, label, shortcut) in COMMANDS {
                    let text = crate::i18n::tr(label);
                    if !format!("{text} {label}").to_lowercase().contains(&query) {
                        continue;
                    }
                    let enter = first
                        && response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    first = false;
                    if ui.button(format!("{text}    {shortcut}")).clicked() || enter {
                        selected = Some(command);
                    }
                }
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        if let Some(command) = selected {
            open = false;
            self.execute_command(ctx, command);
        }
        self.commands.open = open;
        self.commands.was_open = open;
    }
}
