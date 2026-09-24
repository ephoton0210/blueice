// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser-owned native permission confirmation. No page, extension popup,
//! stdin command, or shared-core IPC message can call `key` or `click`: only
//! the real window event handler does. The launcher independently validates
//! the generation, package, and installed optional declaration before any
//! private core mutation, so these controls are not an authorization check.

use super::{draw_rect, draw_text_line, Rect, TAB_STRIP_HEIGHT, TEXT};
use blueice_launcher::control::InstalledExtensionPermissions;
use blueice_launcher::trusted_window::{
    PermissionAction, TrustedWindowReply, TrustedWindowRequest,
};
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, NamedKey};

const PANEL_WIDTH: u32 = 660;
const PANEL_HEIGHT: u32 = 480;
const SCOPE_PAGE_LINES: usize = 7;
const SCOPE_LINE_CHARS: usize = 96;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PanelCommand {
    None,
    Refresh,
    Change(TrustedWindowRequest),
}

/// Package-provided names cannot use invisible or direction-changing text
/// to spoof the native consent chrome. The derived hash remains visible.
pub(super) fn safe_extension_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_control()
                || matches!(character,
                    '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
                    | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

#[derive(Default)]
pub(super) struct PermissionPanel {
    open: bool,
    selected: usize,
    confirming: bool,
    scope_offset: usize,
    notice: Option<String>,
}

impl PermissionPanel {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// F8 is a native window gesture, never a message from the shared core.
    /// Opening always asks for a fresh live inspection before interaction.
    pub(super) fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.confirming = false;
        self.scope_offset = 0;
        self.open
    }

    pub(super) fn invalidate_for_cutover(&mut self) {
        self.confirming = false;
        self.scope_offset = 0;
        self.notice = Some("CORE CHANGED; REINSPECTING PERMISSIONS".into());
    }

    pub(super) fn begin_inspection(&mut self) {
        self.confirming = false;
        self.scope_offset = 0;
        self.notice = None;
    }

    pub(super) fn on_reply(&mut self, reply: &TrustedWindowReply) {
        self.confirming = false;
        match reply {
            TrustedWindowReply::State { installed, .. } => {
                if let Some(installed) = installed {
                    self.selected = self
                        .selected
                        .min(installed.optional.len().saturating_sub(1));
                } else {
                    self.selected = 0;
                }
                self.scope_offset = 0;
                self.notice = None;
            }
            TrustedWindowReply::Rejected { reason } => {
                self.notice = Some(reason.chars().take(160).collect());
            }
        }
    }

    pub(super) fn key(
        &mut self,
        key: &Key,
        reply: Option<&TrustedWindowReply>,
        pending: bool,
    ) -> PanelCommand {
        match key {
            Key::Named(NamedKey::Escape) => {
                if self.confirming {
                    self.confirming = false;
                } else {
                    self.open = false;
                }
                PanelCommand::None
            }
            Key::Named(NamedKey::ArrowUp) if !self.confirming => {
                self.selected = self.selected.saturating_sub(1);
                self.scope_offset = 0;
                PanelCommand::None
            }
            Key::Named(NamedKey::ArrowDown) if !self.confirming => {
                if let Some((_, installed)) = state(reply) {
                    self.selected =
                        (self.selected + 1).min(installed.optional.len().saturating_sub(1));
                    self.scope_offset = 0;
                }
                PanelCommand::None
            }
            Key::Named(NamedKey::PageUp) => {
                self.scope_offset = self.scope_offset.saturating_sub(SCOPE_PAGE_LINES);
                PanelCommand::None
            }
            Key::Named(NamedKey::PageDown) => {
                self.scope_offset = self
                    .scope_offset
                    .saturating_add(SCOPE_PAGE_LINES)
                    .min(self.max_scope_offset(reply));
                PanelCommand::None
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => self.review_or_confirm(reply, pending),
            Key::Character(value) if value.eq_ignore_ascii_case("r") => PanelCommand::Refresh,
            _ => PanelCommand::None,
        }
    }

    pub(super) fn scroll(&mut self, delta: MouseScrollDelta, reply: Option<&TrustedWindowReply>) {
        let up = match delta {
            MouseScrollDelta::LineDelta(_, y) => y > 0.0,
            MouseScrollDelta::PixelDelta(position) => position.y > 0.0,
        };
        if up {
            self.scope_offset = self.scope_offset.saturating_sub(3);
        } else {
            self.scope_offset = self
                .scope_offset
                .saturating_add(3)
                .min(self.max_scope_offset(reply));
        }
    }

    pub(super) fn click(
        &mut self,
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        reply: Option<&TrustedWindowReply>,
        pending: bool,
    ) -> PanelCommand {
        let Some(panel) = panel_rect(width, height) else {
            return PanelCommand::None;
        };
        if button(panel, 500, 439, 135).contains(x, y) {
            self.open = false;
            self.confirming = false;
            return PanelCommand::None;
        }
        if button(panel, 507, 8, 135).contains(x, y) {
            self.confirming = false;
            return PanelCommand::Refresh;
        }
        if button(panel, 20, 439, 190).contains(x, y) {
            return self.review_or_confirm(reply, pending);
        }
        if button(panel, 20, 374, 75).contains(x, y) {
            self.scope_offset = self.scope_offset.saturating_sub(SCOPE_PAGE_LINES);
            return PanelCommand::None;
        }
        if button(panel, 105, 374, 75).contains(x, y) {
            self.scope_offset = self
                .scope_offset
                .saturating_add(SCOPE_PAGE_LINES)
                .min(self.max_scope_offset(reply));
            return PanelCommand::None;
        }
        if self.confirming || pending {
            return PanelCommand::None;
        }
        if let Some((_, installed)) = state(reply) {
            for index in 0..installed.optional.len().min(6) {
                if button(panel, 20, 106 + index as u32 * 22, 620).contains(x, y) {
                    self.selected = index;
                    self.scope_offset = 0;
                    return PanelCommand::None;
                }
            }
        }
        PanelCommand::None
    }

    fn max_scope_offset(&self, reply: Option<&TrustedWindowReply>) -> usize {
        state(reply)
            .and_then(|(_, installed)| installed.optional.get(self.selected))
            .map(|entry| {
                scope_lines(&entry.origins)
                    .len()
                    .saturating_sub(SCOPE_PAGE_LINES)
            })
            .unwrap_or(0)
    }

    fn review_or_confirm(
        &mut self,
        reply: Option<&TrustedWindowReply>,
        pending: bool,
    ) -> PanelCommand {
        if pending {
            return PanelCommand::None;
        }
        let Some((generation, installed)) = state(reply) else {
            return PanelCommand::None;
        };
        let Some(entry) = installed.optional.get(self.selected) else {
            return PanelCommand::None;
        };
        if !self.confirming {
            self.confirming = true;
            self.scope_offset = 0;
            return PanelCommand::None;
        }
        self.confirming = false;
        PanelCommand::Change(TrustedWindowRequest::Change {
            expected_core_generation: generation,
            expected_extension_id: installed.extension_id.clone(),
            capability: entry.capability.clone(),
            action: if entry.granted {
                PermissionAction::Revoke
            } else {
                PermissionAction::Grant
            },
        })
    }

    pub(super) fn draw(
        &self,
        pixels: &mut [u32],
        width: u32,
        height: u32,
        reply: Option<&TrustedWindowReply>,
        pending: bool,
    ) {
        if !self.open {
            return;
        }
        let Some(panel) = panel_rect(width, height) else {
            draw_rect(
                pixels,
                width,
                height,
                Rect {
                    x: 8,
                    y: TAB_STRIP_HEIGHT + 8,
                    width: width.saturating_sub(16),
                    height: 36,
                },
                0x0020_2228,
            );
            draw_text_line(
                pixels,
                width,
                height,
                16,
                TAB_STRIP_HEIGHT + 20,
                "ENLARGE WINDOW TO REVIEW PERMISSIONS",
                48,
                TEXT,
            );
            return;
        };
        draw_rect(pixels, width, height, panel, 0x0016_1C24);
        draw_rect(
            pixels,
            width,
            height,
            Rect {
                x: panel.x + 3,
                y: panel.y + 3,
                width: panel.width - 6,
                height: panel.height - 6,
            },
            0x00F5_F5_EB,
        );
        draw_rect(
            pixels,
            width,
            height,
            Rect {
                x: panel.x + 3,
                y: panel.y + 3,
                width: panel.width - 6,
                height: 27,
            },
            0x003A_526C,
        );
        let title = if self.confirming {
            "CONFIRM EXTENSION PERMISSION CHANGE"
        } else {
            "INSTALLED EXTENSION PERMISSIONS"
        };
        draw_text_line(
            pixels,
            width,
            height,
            panel.x + 12,
            panel.y + 13,
            title,
            70,
            0x00FF_FFFF,
        );
        draw_button(
            pixels,
            width,
            height,
            button(panel, 507, 8, 135),
            "REFRESH",
            0x003A_526C,
        );
        if pending {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 44,
                "WAITING FOR LIVE CORE PERMISSION STATE",
                95,
                0x0020_2228,
            );
        }
        match state(reply) {
            Some((generation, installed)) => {
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + 44,
                    &format!(
                        "{}  VERSION {}  CORE {}",
                        safe_extension_name(&installed.name),
                        safe_extension_name(&installed.version),
                        generation
                    ),
                    98,
                    0x0020_2228,
                );
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + 61,
                    &installed.extension_id,
                    98,
                    0x0020_2228,
                );
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + 86,
                    "OPTIONAL CAPABILITIES (SELECT ONE)",
                    95,
                    0x0020_2228,
                );
                for (index, entry) in installed.optional.iter().take(6).enumerate() {
                    let row = button(panel, 20, 106 + index as u32 * 22, 620);
                    draw_rect(
                        pixels,
                        width,
                        height,
                        row,
                        if self.selected == index {
                            0x00C8_D9_EB
                        } else {
                            0x00E5_E9_EE
                        },
                    );
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        row.x + 8,
                        row.y + 7,
                        &format!(
                            "{}  [{}]",
                            entry.capability,
                            if entry.granted {
                                "GRANTED"
                            } else {
                                "NOT GRANTED"
                            }
                        ),
                        98,
                        0x0020_2228,
                    );
                }
                if let Some(entry) = installed.optional.get(self.selected) {
                    let action = if entry.granted { "REVOKE" } else { "GRANT" };
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        panel.x + 20,
                        panel.y + 251,
                        &format!("{} {}  -  ORIGIN SCOPE:", action, entry.capability),
                        98,
                        0x0020_2228,
                    );
                    let lines = scope_lines(&entry.origins);
                    let max_offset = lines.len().saturating_sub(SCOPE_PAGE_LINES);
                    let offset = self.scope_offset.min(max_offset);
                    for (index, line) in
                        lines.iter().skip(offset).take(SCOPE_PAGE_LINES).enumerate()
                    {
                        draw_text_line(
                            pixels,
                            width,
                            height,
                            panel.x + 20,
                            panel.y + 273 + index as u32 * 13,
                            line,
                            SCOPE_LINE_CHARS,
                            0x0020_2228,
                        );
                    }
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        panel.x + 195,
                        panel.y + 382,
                        &format!(
                            "SCOPE LINES {}-{} OF {}",
                            offset + 1,
                            (offset + SCOPE_PAGE_LINES).min(lines.len()),
                            lines.len()
                        ),
                        65,
                        0x0020_2228,
                    );
                    let label = if self.confirming {
                        "CONFIRM CHANGE"
                    } else {
                        "REVIEW CHANGE"
                    };
                    draw_button(
                        pixels,
                        width,
                        height,
                        button(panel, 20, 439, 190),
                        label,
                        if pending { 0x0080_8790 } else { 0x0023_6B_2A },
                    );
                } else {
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        panel.x + 20,
                        panel.y + 251,
                        "THIS PACKAGE DECLARES NO OPTIONAL CAPABILITIES",
                        95,
                        0x0020_2228,
                    );
                }
            }
            None => {
                let label = if pending {
                    "INSPECTING INSTALLED EXTENSION"
                } else {
                    "NO INSTALLED EXTENSION OR INSPECTION UNAVAILABLE"
                };
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + 60,
                    label,
                    95,
                    0x0020_2228,
                );
            }
        }
        draw_button(
            pixels,
            width,
            height,
            button(panel, 20, 374, 75),
            "PREV",
            0x003A_526C,
        );
        draw_button(
            pixels,
            width,
            height,
            button(panel, 105, 374, 75),
            "NEXT",
            0x003A_526C,
        );
        if pending {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 413,
                "WAITING FOR LAUNCHER CONFIRMATION",
                100,
                0x0020_2228,
            );
        } else if let Some(notice) = &self.notice {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 413,
                notice,
                100,
                0x008B_0000,
            );
        } else if self.confirming {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 413,
                "ONLY CONFIRM IF YOU TRUST THIS PACKAGE AND THE SHOWN SCOPE",
                98,
                0x008B_0000,
            );
        }
        draw_button(
            pixels,
            width,
            height,
            button(panel, 500, 439, 135),
            "CLOSE",
            0x003A_526C,
        );
    }
}

fn state(reply: Option<&TrustedWindowReply>) -> Option<(u64, &InstalledExtensionPermissions)> {
    match reply? {
        TrustedWindowReply::State {
            core_generation,
            installed: Some(installed),
        } => Some((*core_generation, installed)),
        _ => None,
    }
}

fn panel_rect(width: u32, height: u32) -> Option<Rect> {
    (width >= PANEL_WIDTH + 20 && height >= PANEL_HEIGHT + TAB_STRIP_HEIGHT + 12).then(|| Rect {
        x: (width - PANEL_WIDTH) / 2,
        y: (height - PANEL_HEIGHT) / 2,
        width: PANEL_WIDTH,
        height: PANEL_HEIGHT,
    })
}

fn button(panel: Rect, x: u32, y: u32, width: u32) -> Rect {
    Rect {
        x: panel.x + x,
        y: panel.y + y,
        width,
        height: 23,
    }
}

fn draw_button(pixels: &mut [u32], width: u32, height: u32, rect: Rect, label: &str, color: u32) {
    draw_rect(pixels, width, height, rect, color);
    draw_text_line(
        pixels,
        width,
        height,
        rect.x + 8,
        rect.y + 7,
        label,
        32,
        0x00FF_FFFF,
    );
}

/// Wrap every validated origin without eliding a suffix. The native panel
/// pages over the resulting lines, so even a 256-byte origin remains fully
/// inspectable before a person confirms a change.
fn scope_lines(origins: &[String]) -> Vec<String> {
    if origins.is_empty() {
        return vec!["NO ORIGIN RESTRICTION IN MANIFEST".into()];
    }
    let mut lines = Vec::new();
    for (index, origin) in origins.iter().enumerate() {
        let mut chars = origin.chars();
        let mut first = true;
        loop {
            let chunk: String = chars.by_ref().take(SCOPE_LINE_CHARS - 5).collect();
            if chunk.is_empty() {
                break;
            }
            lines.push(format!(
                "{}{}",
                if first {
                    format!("{:02}. ", index + 1)
                } else {
                    "    ".into()
                },
                chunk
            ));
            first = false;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::permission_control::OptionalCapabilityInfo;

    fn installed() -> TrustedWindowReply {
        TrustedWindowReply::State {
            core_generation: 7,
            installed: Some(InstalledExtensionPermissions {
                extension_id: "sha256:installed-package".into(),
                name: "Notes".into(),
                version: "1".into(),
                optional: vec![OptionalCapabilityInfo {
                    capability: "storage".into(),
                    granted: false,
                    origins: vec!["https://example.test".into()],
                }],
            }),
        }
    }

    #[test]
    fn a_native_confirmation_requires_two_separate_actions_and_binds_live_identity() {
        let mut panel = PermissionPanel::default();
        let reply = installed();
        assert!(panel.toggle());
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), Some(&reply), false),
            PanelCommand::None
        );
        assert!(panel.confirming);
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Escape), Some(&reply), false),
            PanelCommand::None
        );
        assert!(!panel.confirming);
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), Some(&reply), false),
            PanelCommand::None
        );
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), Some(&reply), true),
            PanelCommand::None
        );
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), Some(&reply), false),
            PanelCommand::Change(TrustedWindowRequest::Change {
                expected_core_generation: 7,
                expected_extension_id: "sha256:installed-package".into(),
                capability: "storage".into(),
                action: PermissionAction::Grant,
            })
        );
    }

    #[test]
    fn scope_view_wraps_every_byte_of_long_origins_and_cannot_confirm_without_state() {
        let origin = format!("https://{}.test", "a".repeat(220));
        let lines = scope_lines(&[origin.clone()]);
        assert!(lines.len() > 1);
        let reconstructed: String = lines.iter().map(|line| line[4..].to_string()).collect();
        assert_eq!(reconstructed, origin);
        assert!(scope_lines(&vec![origin.clone(); 32]).len() > SCOPE_PAGE_LINES);
        let mut panel = PermissionPanel::default();
        panel.toggle();
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), None, false),
            PanelCommand::None
        );
        let mut reply = installed();
        let TrustedWindowReply::State {
            installed: Some(ref mut package),
            ..
        } = reply
        else {
            unreachable!()
        };
        package.optional[0].origins = vec![origin; 32];
        panel.key(&Key::Named(NamedKey::PageDown), Some(&reply), false);
        assert_eq!(panel.scope_offset, SCOPE_PAGE_LINES);
        for _ in 0..100 {
            panel.key(&Key::Named(NamedKey::PageDown), Some(&reply), false);
        }
        assert_eq!(panel.scope_offset, panel.max_scope_offset(Some(&reply)));
    }

    #[test]
    fn pointer_confirmation_is_two_step_and_small_windows_cannot_send_changes() {
        let mut panel = PermissionPanel::default();
        let reply = installed();
        panel.toggle();
        assert_eq!(
            panel.click(100.0, 500.0, 500, 400, Some(&reply), false),
            PanelCommand::None
        );
        assert!(!panel.confirming);
        let rect = panel_rect(800, 600).unwrap();
        let x = f64::from(rect.x + 25);
        let y = f64::from(rect.y + 445);
        assert_eq!(
            panel.click(x, y, 800, 600, Some(&reply), false),
            PanelCommand::None
        );
        assert!(panel.confirming);
        assert!(matches!(
            panel.click(x, y, 800, 600, Some(&reply), false),
            PanelCommand::Change(TrustedWindowRequest::Change {
                expected_core_generation: 7,
                action: PermissionAction::Grant,
                ..
            })
        ));
    }

    #[test]
    fn native_consent_name_does_not_render_direction_or_invisible_spoofing() {
        assert_eq!(
            safe_extension_name("Safe\u{202e}bad\u{200b}name"),
            "Safe\u{fffd}bad\u{fffd}name"
        );
        assert_eq!(safe_extension_name("中文 Notes"), "中文 Notes");
    }

    #[test]
    fn open_panel_paints_native_chrome_without_changing_pixels_outside_it() {
        let mut panel = PermissionPanel::default();
        panel.toggle();
        let mut pixels = vec![0x00ff_ffff; 800 * 600];
        panel.draw(&mut pixels, 800, 600, Some(&installed()), false);
        let rect = panel_rect(800, 600).unwrap();
        assert_eq!(pixels[0], 0x00ff_ffff);
        assert_ne!(pixels[rect.y as usize * 800 + rect.x as usize], 0x00ff_ffff);
        let mut small = vec![0x00ff_ffff; 500 * 400];
        panel.draw(&mut small, 500, 400, Some(&installed()), false);
        assert_eq!(small[0], 0x00ff_ffff);
    }
}
