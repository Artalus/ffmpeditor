use egui::{Context, Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Import,
    AddText,
    Export,
    Quit,
    Undo,
    Redo,
    Split,
    DeleteClip,
    GoToStart,
    GoToEnd,
    PreviousFrame,
    NextFrame,
    ProjectSettings,
    Settings,
}

/// What the menus need to know to grey out commands that would do nothing.
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub clip_selected: bool,
    pub export_running: bool,
}

/// The first shortcut of a command is the one shown in menus.
/// egui matches Ctrl+S when Ctrl+Shift+S is pressed, so shortcuts with more modifiers go first.
const BINDINGS: &[(Command, &[KeyboardShortcut])] = &[
    (Command::SaveAs, &[ctrl_shift(Key::S)]),
    (Command::Redo, &[ctrl(Key::Y), ctrl_shift(Key::Z)]),
    (Command::New, &[ctrl(Key::N)]),
    (Command::Open, &[ctrl(Key::O)]),
    (Command::Save, &[ctrl(Key::S)]),
    (Command::Import, &[ctrl(Key::I)]),
    (Command::AddText, &[ctrl(Key::T)]),
    (Command::Export, &[ctrl(Key::E)]),
    (Command::Undo, &[ctrl(Key::Z)]),
    (Command::Split, &[plain(Key::S)]),
    (Command::DeleteClip, &[plain(Key::Delete)]),
    (Command::GoToStart, &[plain(Key::Home)]),
    (Command::GoToEnd, &[plain(Key::End)]),
    (Command::PreviousFrame, &[plain(Key::ArrowLeft)]),
    (Command::NextFrame, &[plain(Key::ArrowRight)]),
];

pub fn read_shortcuts(ctx: &Context) -> Vec<Command> {
    ctx.input_mut(|input| {
        BINDINGS
            .iter()
            .flat_map(|(command, shortcuts)| shortcuts.iter().map(move |s| (s, *command)))
            .filter(|(shortcut, _)| {
                // egui ignores extra Shift and Alt, so a bare S would also fire on Shift+S.
                let held_ok = !shortcut.modifiers.is_none() || input.modifiers.is_none();
                held_ok && input.consume_shortcut(shortcut)
            })
            .map(|(_, command)| command)
            .collect()
    })
}

pub fn menu_bar(ui: &mut egui::Ui, state: &MenuState) -> Option<Command> {
    let mut command = None;
    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item(ui, &mut command, Command::New, true);
                item(ui, &mut command, Command::Open, true);
                item(ui, &mut command, Command::Save, true);
                item(ui, &mut command, Command::SaveAs, true);
                ui.separator();
                item(ui, &mut command, Command::Export, !state.export_running);
                ui.separator();
                item(ui, &mut command, Command::Quit, true);
            });
            ui.menu_button("Edit", |ui| {
                item(ui, &mut command, Command::Undo, state.can_undo);
                item(ui, &mut command, Command::Redo, state.can_redo);
                ui.separator();
                item(ui, &mut command, Command::Split, true);
                item(ui, &mut command, Command::DeleteClip, state.clip_selected);
                ui.separator();
                item(ui, &mut command, Command::GoToStart, true);
                item(ui, &mut command, Command::GoToEnd, true);
                item(ui, &mut command, Command::PreviousFrame, true);
                item(ui, &mut command, Command::NextFrame, true);
            });
            ui.menu_button("Media", |ui| {
                item(ui, &mut command, Command::Import, true);
                item(ui, &mut command, Command::AddText, true);
            });
            ui.menu_button("Project", |ui| {
                item(ui, &mut command, Command::ProjectSettings, true);
            });
            ui.menu_button("Options", |ui| {
                item(ui, &mut command, Command::Settings, true);
            });
        });
    });
    command
}

fn item(ui: &mut egui::Ui, clicked: &mut Option<Command>, command: Command, enabled: bool) {
    let mut button = egui::Button::new(command.label());
    if let Some(shortcut) = shortcut(command) {
        button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
    }
    if ui.add_enabled(enabled, button).clicked() {
        *clicked = Some(command);
    }
}

fn shortcut(command: Command) -> Option<&'static KeyboardShortcut> {
    BINDINGS
        .iter()
        .find(|(c, _)| *c == command)
        .and_then(|(_, shortcuts)| shortcuts.first())
}

const fn plain(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, key)
}

const fn ctrl(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, key)
}

const fn ctrl_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Command::New => "New",
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::SaveAs => "Save as…",
            Command::Import => "Import media…",
            Command::AddText => "Add text",
            Command::Export => "Export…",
            Command::Quit => "Quit",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Split => "Split at playhead",
            Command::DeleteClip => "Delete selected clip",
            Command::GoToStart => "Go to start",
            Command::GoToEnd => "Go to end",
            Command::PreviousFrame => "Previous frame",
            Command::NextFrame => "Next frame",
            Command::ProjectSettings => "Project settings…",
            Command::Settings => "Settings…",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(key: Key, modifiers: Modifiers) -> Vec<Command> {
        let event = egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let input = egui::RawInput {
            events: vec![egui::Event::ModifiersChanged(modifiers), event],
            ..Default::default()
        };
        let mut commands = Vec::new();
        Context::default()
            .run_pass(input, |ctx| commands = read_shortcuts(ctx))
            .drop_without_applying_deltas();
        commands
    }

    #[test]
    fn shortcuts_match_their_exact_modifiers() {
        assert_eq!(pressed(Key::S, Modifiers::NONE), [Command::Split]);
        assert_eq!(pressed(Key::S, Modifiers::SHIFT), []);
        assert_eq!(pressed(Key::S, Modifiers::ALT), []);
        assert_eq!(pressed(Key::Delete, Modifiers::SHIFT), []);
        assert_eq!(pressed(Key::S, Modifiers::COMMAND), [Command::Save]);
        assert_eq!(
            pressed(Key::S, Modifiers::COMMAND | Modifiers::SHIFT),
            [Command::SaveAs]
        );
        assert_eq!(pressed(Key::Z, Modifiers::COMMAND), [Command::Undo]);
        assert_eq!(
            pressed(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
            [Command::Redo]
        );
    }
}
