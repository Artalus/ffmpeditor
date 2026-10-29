use std::cmp::Reverse;

use egui::{Context, Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Import,
    Quit,
    Undo,
    Redo,
    Split,
    DeleteClip,
    GoToStart,
    GoToEnd,
    PreviousFrame,
    NextFrame,
}

/// What the menus need to know to grey out commands that would do nothing.
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub clip_selected: bool,
}

/// The first shortcut of a command is the one shown in menus.
const BINDINGS: &[(Command, &[KeyboardShortcut])] = &[
    (Command::New, &[ctrl(Key::N)]),
    (Command::Open, &[ctrl(Key::O)]),
    (Command::Save, &[ctrl(Key::S)]),
    (Command::SaveAs, &[ctrl_shift(Key::S)]),
    (Command::Import, &[ctrl(Key::I)]),
    (Command::Undo, &[ctrl(Key::Z)]),
    (Command::Redo, &[ctrl(Key::Y), ctrl_shift(Key::Z)]),
    (Command::Split, &[plain(Key::S)]),
    (Command::DeleteClip, &[plain(Key::Delete)]),
    (Command::GoToStart, &[plain(Key::Home)]),
    (Command::GoToEnd, &[plain(Key::End)]),
    (Command::PreviousFrame, &[plain(Key::ArrowLeft)]),
    (Command::NextFrame, &[plain(Key::ArrowRight)]),
];

pub fn read_shortcuts(ctx: &Context) -> Vec<Command> {
    ctx.input_mut(|input| {
        matching_order()
            .into_iter()
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
                item(ui, &mut command, Command::Import, true);
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
        });
    });
    command
}

/// egui matches Ctrl+S when Ctrl+Shift+S is pressed, so shortcuts with more modifiers go first.
fn matching_order() -> Vec<(KeyboardShortcut, Command)> {
    let mut order: Vec<_> = BINDINGS
        .iter()
        .flat_map(|(command, shortcuts)| shortcuts.iter().map(|shortcut| (*shortcut, *command)))
        .collect();
    order.sort_by_key(|(shortcut, _)| Reverse(modifier_count(shortcut.modifiers)));
    order
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

fn modifier_count(modifiers: Modifiers) -> u8 {
    u8::from(modifiers.alt)
        + u8::from(modifiers.shift)
        + u8::from(modifiers.ctrl || modifiers.command || modifiers.mac_cmd)
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
            Command::Quit => "Quit",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Split => "Split at playhead",
            Command::DeleteClip => "Delete selected clip",
            Command::GoToStart => "Go to start",
            Command::GoToEnd => "Go to end",
            Command::PreviousFrame => "Previous frame",
            Command::NextFrame => "Next frame",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(shortcut: KeyboardShortcut) -> usize {
        matching_order()
            .iter()
            .position(|(s, _)| *s == shortcut)
            .unwrap()
    }

    #[test]
    fn specific_shortcuts_match_first() {
        assert!(position(ctrl_shift(Key::Z)) < position(ctrl(Key::Z)));
        assert!(position(ctrl_shift(Key::S)) < position(ctrl(Key::S)));
        assert!(position(ctrl(Key::S)) < position(plain(Key::S)));
    }

    #[test]
    fn shortcuts_are_unique() {
        let order = matching_order();
        for (i, (a, _)) in order.iter().enumerate() {
            assert!(order[i + 1..].iter().all(|(b, _)| a != b), "{a:?}");
        }
    }

    #[test]
    fn menus_show_first_shortcut() {
        assert_eq!(shortcut(Command::Redo), Some(&ctrl(Key::Y)));
        assert_eq!(shortcut(Command::Quit), None);
    }

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
    fn bare_keys_ignore_shift_and_alt() {
        assert_eq!(pressed(Key::S, Modifiers::NONE), [Command::Split]);
        assert_eq!(pressed(Key::S, Modifiers::SHIFT), []);
        assert_eq!(pressed(Key::S, Modifiers::ALT), []);
        assert_eq!(pressed(Key::Delete, Modifiers::SHIFT), []);
        assert_eq!(pressed(Key::S, Modifiers::COMMAND), [Command::Save]);
        assert_eq!(
            pressed(Key::S, Modifiers::COMMAND | Modifiers::SHIFT),
            [Command::SaveAs]
        );
    }
}
