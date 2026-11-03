// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reference frontend's address field: the editable text and the rules
//! that turn typed text into a URL to navigate to. Drawing and window events
//! stay in `main.rs`; this holds only state a test can drive.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// What a key press asks the window to do.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AddressCommand {
    /// Nothing changed.
    None,
    /// The text or focus changed; repaint the field.
    Redraw,
    /// Go to this URL in the selected tab.
    Navigate(String),
}

#[derive(Default)]
pub(super) struct AddressBar {
    editing: Option<Editing>,
}

struct Editing {
    text: String,
    /// The field was just focused, so the first edit replaces the old URL
    /// rather than extending it (the effect of selecting all).
    replace_all: bool,
}

impl AddressBar {
    pub(super) fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Starts editing with `current` (the selected tab's URL) selected.
    pub(super) fn focus(&mut self, current: &str) {
        self.editing = Some(Editing {
            text: current.to_string(),
            replace_all: true,
        });
    }

    pub(super) fn blur(&mut self) {
        self.editing = None;
    }

    /// The text to show: the edit buffer while editing, otherwise `current`.
    pub(super) fn display<'a>(&'a self, current: &'a str) -> &'a str {
        self.editing
            .as_ref()
            .map_or(current, |editing| editing.text.as_str())
    }

    /// Whether the whole text is shown selected.
    pub(super) fn is_selected(&self) -> bool {
        self.editing.as_ref().is_some_and(|e| e.replace_all)
    }

    /// Applies a key press while the field has focus.
    pub(super) fn key(&mut self, key: &Key, text: Option<&str>) -> AddressCommand {
        let Some(editing) = self.editing.as_mut() else {
            return AddressCommand::None;
        };
        match key {
            Key::Named(NamedKey::Enter) => match normalize(&editing.text) {
                Some(url) => {
                    self.editing = None;
                    AddressCommand::Navigate(url)
                }
                None => AddressCommand::None,
            },
            Key::Named(NamedKey::Escape) => {
                self.editing = None;
                AddressCommand::Redraw
            }
            Key::Named(NamedKey::Backspace) => {
                if editing.replace_all {
                    editing.text.clear();
                    editing.replace_all = false;
                } else if editing.text.pop().is_none() {
                    return AddressCommand::None;
                }
                AddressCommand::Redraw
            }
            _ => {
                let Some(text) = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                else {
                    return AddressCommand::None;
                };
                if editing.replace_all {
                    editing.text.clear();
                    editing.replace_all = false;
                }
                editing.text.push_str(text);
                AddressCommand::Redraw
            }
        }
    }
}

/// Command-L (macOS) or Control-L: focus the address field.
pub(super) fn is_focus_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    (modifiers.super_key() || modifiers.control_key())
        && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("l"))
}

/// Turns typed text into a URL: a scheme-less address gets `https://`, a
/// full URL or an `about:` page is kept, and anything that is not a single
/// token (empty, or with spaces) is not an address.
pub(super) fn normalize(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return None;
    }
    if input.contains("://") || input.starts_with("about:") {
        Some(input.to_string())
    } else {
        Some(format!("https://{input}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    fn type_text(bar: &mut AddressBar, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            bar.key(&character(&s), Some(&s));
        }
    }

    #[test]
    fn a_bare_host_gets_https_and_a_full_url_is_kept() {
        assert_eq!(
            normalize("tw.yahoo.com").as_deref(),
            Some("https://tw.yahoo.com")
        );
        assert_eq!(
            normalize("  example.com/a?b=1 ").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(
            normalize("http://localhost:8080/").as_deref(),
            Some("http://localhost:8080/")
        );
        assert_eq!(
            normalize("localhost:8080").as_deref(),
            Some("https://localhost:8080")
        );
        assert_eq!(normalize("about:credits").as_deref(), Some("about:credits"));
    }

    #[test]
    fn empty_or_spaced_text_is_not_an_address() {
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("   "), None);
        assert_eq!(normalize("two words"), None);
    }

    #[test]
    fn focusing_selects_the_current_url_and_typing_replaces_it() {
        let mut bar = AddressBar::default();
        assert!(!bar.is_editing());
        assert_eq!(bar.display("https://old/"), "https://old/");
        bar.focus("https://old/");
        assert!(bar.is_editing() && bar.is_selected());
        assert_eq!(bar.display("ignored"), "https://old/");
        assert_eq!(bar.key(&character("n"), Some("n")), AddressCommand::Redraw);
        assert!(!bar.is_selected());
        type_text(&mut bar, "ew.org");
        assert_eq!(bar.display(""), "new.org");
    }

    #[test]
    fn backspace_clears_a_selection_then_removes_one_character() {
        let mut bar = AddressBar::default();
        bar.focus("abc");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Backspace), None),
            AddressCommand::Redraw
        );
        assert_eq!(bar.display("x"), "");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Backspace), None),
            AddressCommand::None
        );
        type_text(&mut bar, "ab");
        bar.key(&Key::Named(NamedKey::Backspace), None);
        assert_eq!(bar.display(""), "a");
    }

    #[test]
    fn enter_navigates_to_the_normalized_text_and_leaves_the_field() {
        let mut bar = AddressBar::default();
        bar.focus("");
        type_text(&mut bar, "example.com");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Enter), None),
            AddressCommand::Navigate("https://example.com".to_string())
        );
        assert!(!bar.is_editing());
    }

    #[test]
    fn enter_on_something_that_is_not_an_address_stays_in_the_field() {
        let mut bar = AddressBar::default();
        bar.focus("");
        type_text(&mut bar, "two words");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Enter), None),
            AddressCommand::None
        );
        assert!(bar.is_editing());
    }

    #[test]
    fn escape_abandons_the_edit() {
        let mut bar = AddressBar::default();
        bar.focus("https://old/");
        type_text(&mut bar, "zzz");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Escape), None),
            AddressCommand::Redraw
        );
        assert!(!bar.is_editing());
        assert_eq!(bar.display("https://old/"), "https://old/");
    }

    #[test]
    fn keys_are_ignored_without_focus_and_control_text_is_not_inserted() {
        let mut bar = AddressBar::default();
        assert_eq!(bar.key(&character("a"), Some("a")), AddressCommand::None);
        bar.focus("x");
        assert_eq!(
            bar.key(&Key::Named(NamedKey::Tab), Some("\t")),
            AddressCommand::None
        );
        assert_eq!(bar.key(&character("a"), Some("")), AddressCommand::None);
        assert_eq!(bar.key(&character("a"), None), AddressCommand::None);
        assert_eq!(bar.display(""), "x");
    }

    #[test]
    fn command_or_control_l_is_the_focus_shortcut() {
        assert!(is_focus_shortcut(&character("l"), ModifiersState::SUPER));
        assert!(is_focus_shortcut(&character("L"), ModifiersState::CONTROL));
        assert!(!is_focus_shortcut(&character("l"), ModifiersState::empty()));
        assert!(!is_focus_shortcut(&character("k"), ModifiersState::SUPER));
        assert!(!is_focus_shortcut(
            &Key::Named(NamedKey::Enter),
            ModifiersState::SUPER
        ));
    }
}
