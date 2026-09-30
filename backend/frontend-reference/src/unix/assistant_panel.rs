// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browser-owned native assistant-settings panel (`phase-7-local-ai/PLAN.md`,
//! step R8). Like [`super::permission_panel`], nothing but the real window
//! event handler can call `key` or `click`: no page, stdin command, MCP tool, or
//! shared-core IPC message reaches it, and its requests travel only on the
//! launcher's private trusted-window pipe. That is the whole point of putting
//! the editor here: an AI agent can *propose* a settings change over MCP, but
//! only the person, in this window, can approve it.
//!
//! Two jobs:
//!
//! * **Review** an agent's pending proposal as `label: before -> after` lines
//!   and approve or deny it. Approval is two-step and bound to the proposal the
//!   person was looking at: the confirmation carries the id and digest of the
//!   proposal on screen, and any fresh state from the launcher cancels a
//!   half-made confirmation, so a proposal swapped underneath cannot inherit an
//!   approval.
//! * **Edit** the numeric settings directly (backend, idle timeout, memory
//!   ceiling, priority, candle context) with steppers, and apply them in two
//!   steps. Paths and model names need a text field this window does not have,
//!   so they stay in the settings file; the panel says so.
//!
//! The launcher validates everything again; these controls are not the
//! authorization check. All text an agent influenced (the diff lines) is
//! stripped of control and direction-changing characters before it is drawn, so
//! a proposal cannot spoof the consent chrome.

use super::permission_panel::{button, draw_button, panel_rect, safe_extension_name};
use super::{draw_rect, draw_text_line, Rect, TAB_STRIP_HEIGHT, TEXT};
use blueice_assistant_settings::{
    AssistantSettings, BackendKind, MAX_CANDLE_CONTEXT, MAX_IDLE_TIMEOUT_SECS, MAX_NICE,
    MIN_CANDLE_CONTEXT, MIN_IDLE_TIMEOUT_SECS, MIN_RESIDENT_MB,
};
use blueice_launcher::trusted_window::{
    PendingAssistantProposal, TrustedWindowReply, TrustedWindowRequest,
};
use winit::keyboard::{Key, NamedKey};

const DIFF_LINES_SHOWN: usize = 6;
const LINE_CHARS: usize = 96;
const ROW_TOP: u32 = 176;
const ROW_STEP: u32 = 26;
const IDLE_STEP: u64 = 30;
const CEILING_STEP: u64 = 256;
/// The ceiling a person gets when they first step up from "no limit".
const CEILING_FIRST: u64 = 2048;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AssistantCommand {
    None,
    /// Send this request on the private trusted-window pipe.
    Send(TrustedWindowRequest),
}

/// A two-step action awaiting its second, confirming gesture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Confirm {
    #[default]
    None,
    /// Approving this exact proposal: the id and digest that were on screen.
    Approve { id: u64, digest: String },
    /// Applying the drafted edits.
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Backend,
    Idle,
    Ceiling,
    Nice,
    CandleContext,
}

/// The rows shown for `settings`: candle context only when there is a candle
/// section to change.
fn rows(settings: &AssistantSettings) -> Vec<Row> {
    let mut rows = vec![Row::Backend, Row::Idle, Row::Ceiling, Row::Nice];
    if settings.candle.is_some() {
        rows.push(Row::CandleContext);
    }
    rows
}

/// The backends `settings` has the sections for, in cycling order.
fn available_backends(settings: &AssistantSettings) -> Vec<BackendKind> {
    let mut backends = vec![BackendKind::None];
    if settings.loopback.is_some() {
        backends.push(BackendKind::Loopback);
    }
    if settings.candle.is_some() {
        backends.push(BackendKind::Candle);
    }
    if settings.loopback.is_some() && settings.candle.is_some() {
        backends.push(BackendKind::Both);
    }
    backends
}

/// Steps `row` of `draft` one notch (`up == true` increases). Returns whether
/// anything changed, so a stepper at its limit is a no-op rather than a wrap.
/// The result always satisfies the same bounds `AssistantSettings::validate`
/// enforces, so the person cannot draft something the launcher would reject on
/// range grounds alone.
fn adjust(draft: &mut AssistantSettings, row: Row, up: bool) -> bool {
    match row {
        Row::Backend => {
            let backends = available_backends(draft);
            let at = backends
                .iter()
                .position(|b| *b == draft.backend)
                .unwrap_or(0);
            let next = if up { at + 1 } else { at.wrapping_sub(1) };
            match backends.get(next) {
                Some(next) => {
                    draft.backend = *next;
                    true
                }
                None => false,
            }
        }
        Row::Idle => {
            let next = if up {
                (draft.idle_timeout_secs + IDLE_STEP).min(MAX_IDLE_TIMEOUT_SECS)
            } else {
                draft
                    .idle_timeout_secs
                    .saturating_sub(IDLE_STEP)
                    .max(MIN_IDLE_TIMEOUT_SECS)
            };
            std::mem::replace(&mut draft.idle_timeout_secs, next) != next
        }
        Row::Ceiling => {
            let next = match (draft.max_resident_mb, up) {
                (None, true) => Some(CEILING_FIRST),
                (None, false) => None,
                (Some(mb), true) => {
                    Some((mb + CEILING_STEP).min(blueice_assistant_settings::MAX_RESIDENT_MB))
                }
                // Stepping down from the minimum removes the limit.
                (Some(mb), false) if mb <= MIN_RESIDENT_MB => None,
                (Some(mb), false) => Some((mb - CEILING_STEP).max(MIN_RESIDENT_MB)),
            };
            std::mem::replace(&mut draft.max_resident_mb, next) != next
        }
        Row::Nice => {
            let next = if up {
                (draft.nice + 1).min(MAX_NICE)
            } else {
                (draft.nice - 1).max(0)
            };
            std::mem::replace(&mut draft.nice, next) != next
        }
        Row::CandleContext => {
            let Some(candle) = draft.candle.as_mut() else {
                return false;
            };
            let next = if up {
                (candle.context * 2).min(MAX_CANDLE_CONTEXT)
            } else {
                (candle.context / 2).max(MIN_CANDLE_CONTEXT)
            };
            std::mem::replace(&mut candle.context, next) != next
        }
    }
}

fn row_label(row: Row) -> &'static str {
    match row {
        Row::Backend => "BACKEND",
        Row::Idle => "IDLE TIMEOUT (S)",
        Row::Ceiling => "MEMORY CEILING (MIB)",
        Row::Nice => "PRIORITY (NICE)",
        Row::CandleContext => "CANDLE CONTEXT",
    }
}

fn row_value(settings: &AssistantSettings, row: Row) -> String {
    match row {
        Row::Backend => format!("{:?}", settings.backend).to_uppercase(),
        Row::Idle => settings.idle_timeout_secs.to_string(),
        Row::Ceiling => settings
            .max_resident_mb
            .map_or("NO LIMIT".into(), |mb| mb.to_string()),
        Row::Nice => settings.nice.to_string(),
        Row::CandleContext => settings
            .candle
            .as_ref()
            .map_or("-".into(), |c| c.context.to_string()),
    }
}

/// Text an agent influenced, made safe to draw inside the consent chrome:
/// control and direction-changing characters are replaced (the same rule the
/// permission panel applies to extension names) and the line is bounded.
fn safe_line(text: &str) -> String {
    safe_extension_name(text)
        .chars()
        .take(LINE_CHARS)
        .collect::<String>()
        .to_uppercase()
}

#[derive(Default)]
pub(super) struct AssistantPanel {
    open: bool,
    current: Option<AssistantSettings>,
    pending: Option<PendingAssistantProposal>,
    draft: Option<AssistantSettings>,
    row: usize,
    confirm: Confirm,
    notice: Option<String>,
}

impl AssistantPanel {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// The window gesture (F9). Opening always asks for a fresh state.
    pub(super) fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.confirm = Confirm::None;
        self.notice = None;
        self.open
    }

    /// A reply from the launcher. Fresh state discards any half-made
    /// confirmation and any unapplied draft: what is on screen must be what the
    /// launcher just said is true.
    pub(super) fn on_reply(&mut self, reply: &TrustedWindowReply) {
        self.confirm = Confirm::None;
        match reply {
            TrustedWindowReply::AssistantSettingsState { current, pending } => {
                self.current = Some((**current).clone());
                self.draft = Some((**current).clone());
                self.pending = pending.as_deref().cloned();
                self.row = self.row.min(rows(current).len().saturating_sub(1));
                self.notice = None;
            }
            TrustedWindowReply::Rejected { reason } => {
                self.notice = Some(reason.chars().take(160).collect());
            }
            _ => {}
        }
    }

    fn dirty(&self) -> bool {
        matches!((&self.current, &self.draft), (Some(c), Some(d)) if c != d)
    }

    fn rows(&self) -> Vec<Row> {
        self.draft.as_ref().map(rows).unwrap_or_default()
    }

    fn begin_approve(&mut self, in_flight: bool) {
        if in_flight {
            return;
        }
        if let Some(pending) = &self.pending {
            // Bound to what is on screen right now.
            self.confirm = Confirm::Approve {
                id: pending.id,
                digest: pending.digest.clone(),
            };
        }
    }

    fn begin_apply(&mut self, in_flight: bool) {
        if !in_flight && self.dirty() {
            self.confirm = Confirm::Apply;
        }
    }

    fn confirm_now(&mut self, in_flight: bool) -> AssistantCommand {
        if in_flight {
            return AssistantCommand::None;
        }
        match std::mem::take(&mut self.confirm) {
            Confirm::Approve { id, digest } => {
                // The proposal must still be the one that was confirmed.
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.id == id && p.digest == digest)
                {
                    AssistantCommand::Send(TrustedWindowRequest::ApproveAssistantProposal {
                        id,
                        digest,
                    })
                } else {
                    self.notice = Some("THE PROPOSAL CHANGED; REVIEW IT AGAIN".into());
                    AssistantCommand::None
                }
            }
            Confirm::Apply => match self.draft.clone() {
                Some(settings) if self.dirty() => {
                    AssistantCommand::Send(TrustedWindowRequest::EditAssistantSettings { settings })
                }
                _ => AssistantCommand::None,
            },
            Confirm::None => AssistantCommand::None,
        }
    }

    fn deny(&mut self, in_flight: bool) -> AssistantCommand {
        match (&self.pending, in_flight) {
            (Some(pending), false) => {
                self.confirm = Confirm::None;
                AssistantCommand::Send(TrustedWindowRequest::DenyAssistantProposal {
                    id: pending.id,
                })
            }
            _ => AssistantCommand::None,
        }
    }

    fn step(&mut self, up: bool) {
        if self.confirm != Confirm::None {
            return;
        }
        let rows = self.rows();
        if let (Some(draft), Some(row)) = (self.draft.as_mut(), rows.get(self.row)) {
            adjust(draft, *row, up);
        }
    }

    pub(super) fn key(&mut self, key: &Key, in_flight: bool, repeat: bool) -> AssistantCommand {
        match key {
            Key::Named(NamedKey::Escape) => {
                if self.confirm != Confirm::None {
                    self.confirm = Confirm::None;
                } else {
                    self.open = false;
                }
                AssistantCommand::None
            }
            Key::Named(NamedKey::ArrowUp) if self.confirm == Confirm::None => {
                self.row = self.row.saturating_sub(1);
                AssistantCommand::None
            }
            Key::Named(NamedKey::ArrowDown) if self.confirm == Confirm::None => {
                self.row = (self.row + 1).min(self.rows().len().saturating_sub(1));
                AssistantCommand::None
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.step(false);
                AssistantCommand::None
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.step(true);
                AssistantCommand::None
            }
            // A held key must never walk through both steps of a confirmation.
            Key::Named(NamedKey::Enter) if !repeat => self.confirm_now(in_flight),
            Key::Character(c) if !repeat && c.eq_ignore_ascii_case("a") => {
                self.begin_approve(in_flight);
                AssistantCommand::None
            }
            Key::Character(c) if !repeat && c.eq_ignore_ascii_case("d") => self.deny(in_flight),
            Key::Character(c) if !repeat && c.eq_ignore_ascii_case("e") => {
                self.begin_apply(in_flight);
                AssistantCommand::None
            }
            Key::Character(c) if c.eq_ignore_ascii_case("r") && !in_flight => {
                AssistantCommand::Send(TrustedWindowRequest::InspectAssistantSettings)
            }
            _ => AssistantCommand::None,
        }
    }

    pub(super) fn click(
        &mut self,
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        in_flight: bool,
    ) -> AssistantCommand {
        let Some(panel) = panel_rect(width, height) else {
            return AssistantCommand::None;
        };
        if button(panel, 550, 439, 90).contains(x, y) {
            self.open = false;
            self.confirm = Confirm::None;
            return AssistantCommand::None;
        }
        if self.confirm != Confirm::None {
            if button(panel, 20, 439, 190).contains(x, y) {
                return self.confirm_now(in_flight);
            }
            if button(panel, 230, 439, 190).contains(x, y) {
                self.confirm = Confirm::None;
            }
            return AssistantCommand::None;
        }
        if in_flight {
            return AssistantCommand::None;
        }
        if button(panel, 20, 439, 130).contains(x, y) {
            self.begin_approve(false);
        } else if button(panel, 160, 439, 90).contains(x, y) {
            return self.deny(false);
        } else if button(panel, 260, 439, 120).contains(x, y) {
            self.begin_apply(false);
        } else if button(panel, 390, 439, 100).contains(x, y) {
            return AssistantCommand::Send(TrustedWindowRequest::InspectAssistantSettings);
        } else if let Some(hit) = self.stepper_at(panel, x, y) {
            self.row = hit.0;
            self.step(hit.1);
        }
        AssistantCommand::None
    }

    /// Which row's `-`/`+` button (if either) is at `(x, y)`.
    fn stepper_at(&self, panel: Rect, x: f64, y: f64) -> Option<(usize, bool)> {
        (0..self.rows().len()).find_map(|index| {
            let top = ROW_TOP + index as u32 * ROW_STEP;
            if button(panel, 470, top, 40).contains(x, y) {
                Some((index, false))
            } else if button(panel, 520, top, 40).contains(x, y) {
                Some((index, true))
            } else {
                None
            }
        })
    }

    pub(super) fn draw(&self, pixels: &mut [u32], width: u32, height: u32, in_flight: bool) {
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
                "ENLARGE WINDOW TO REVIEW ASSISTANT SETTINGS",
                48,
                TEXT,
            );
            return;
        };
        let ink = 0x0020_2228;
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
            0x00F5_F5EB,
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
        let title = match self.confirm {
            Confirm::Approve { .. } => "CONFIRM: APPROVE THIS AI PROPOSAL",
            Confirm::Apply => "CONFIRM: APPLY THESE SETTINGS",
            Confirm::None => "ASSISTANT SETTINGS",
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
        if in_flight {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 34,
                "WAITING FOR THE LAUNCHER",
                60,
                ink,
            );
        }

        match &self.pending {
            Some(pending) => {
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + 44,
                    &format!(
                        "AN AI AGENT PROPOSES (EXPIRES IN {} S):",
                        pending.seconds_left
                    ),
                    90,
                    0x008B_0000,
                );
                for (index, line) in pending.diff.iter().take(DIFF_LINES_SHOWN).enumerate() {
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        panel.x + 28,
                        panel.y + 62 + index as u32 * 14,
                        &safe_line(line),
                        LINE_CHARS,
                        ink,
                    );
                }
                if pending.diff.len() > DIFF_LINES_SHOWN {
                    draw_text_line(
                        pixels,
                        width,
                        height,
                        panel.x + 28,
                        panel.y + 62 + DIFF_LINES_SHOWN as u32 * 14,
                        &format!("... AND {} MORE", pending.diff.len() - DIFF_LINES_SHOWN),
                        40,
                        ink,
                    );
                }
            }
            None => draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 44,
                "NO AI PROPOSAL IS WAITING",
                60,
                ink,
            ),
        }

        draw_text_line(
            pixels,
            width,
            height,
            panel.x + 20,
            panel.y + 158,
            "YOUR SETTINGS (PATHS AND MODEL NAMES ARE EDITED IN THE SETTINGS FILE)",
            90,
            ink,
        );
        if let Some(draft) = &self.draft {
            for (index, row) in rows(draft).into_iter().enumerate() {
                let top = ROW_TOP + index as u32 * ROW_STEP;
                let marker = if index == self.row { ">" } else { " " };
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 20,
                    panel.y + top + 7,
                    &format!("{marker} {}", row_label(row)),
                    30,
                    ink,
                );
                draw_text_line(
                    pixels,
                    width,
                    height,
                    panel.x + 260,
                    panel.y + top + 7,
                    &row_value(draft, row),
                    24,
                    ink,
                );
                draw_button(
                    pixels,
                    width,
                    height,
                    button(panel, 470, top, 40),
                    "-",
                    0x003A_526C,
                );
                draw_button(
                    pixels,
                    width,
                    height,
                    button(panel, 520, top, 40),
                    "+",
                    0x003A_526C,
                );
            }
        }
        if let Some(notice) = &self.notice {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 413,
                &safe_line(notice),
                100,
                0x008B_0000,
            );
        }

        if self.confirm != Confirm::None {
            draw_text_line(
                pixels,
                width,
                height,
                panel.x + 20,
                panel.y + 395,
                "THIS TAKES EFFECT IMMEDIATELY AND RESTARTS THE ASSISTANT",
                90,
                0x008B_0000,
            );
            draw_button(
                pixels,
                width,
                height,
                button(panel, 20, 439, 190),
                "CONFIRM",
                if in_flight { 0x0080_8790 } else { 0x008B_0000 },
            );
            draw_button(
                pixels,
                width,
                height,
                button(panel, 230, 439, 190),
                "CANCEL",
                0x003A_526C,
            );
        } else {
            let idle = in_flight;
            let enabled = |on: bool| {
                if on && !idle {
                    0x008B_0000
                } else {
                    0x0080_8790
                }
            };
            draw_button(
                pixels,
                width,
                height,
                button(panel, 20, 439, 130),
                "APPROVE",
                enabled(self.pending.is_some()),
            );
            draw_button(
                pixels,
                width,
                height,
                button(panel, 160, 439, 90),
                "DENY",
                enabled(self.pending.is_some()),
            );
            draw_button(
                pixels,
                width,
                height,
                button(panel, 260, 439, 120),
                "APPLY EDITS",
                enabled(self.dirty()),
            );
            draw_button(
                pixels,
                width,
                height,
                button(panel, 390, 439, 100),
                "REFRESH",
                0x003A_526C,
            );
        }
        draw_button(
            pixels,
            width,
            height,
            button(panel, 550, 439, 90),
            "CLOSE",
            0x003A_526C,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_assistant_settings::{CandleSettings, LoopbackSettings};

    fn settings() -> AssistantSettings {
        AssistantSettings {
            backend: BackendKind::Loopback,
            loopback: Some(LoopbackSettings {
                provider: "llamacpp".into(),
                base_url: "http://127.0.0.1:8080/v1/".into(),
                model: "m".into(),
            }),
            max_resident_mb: Some(2048),
            ..AssistantSettings::default()
        }
    }

    fn with_candle(mut s: AssistantSettings) -> AssistantSettings {
        s.candle = Some(CandleSettings {
            model_path: "/m/a.gguf".into(),
            tokenizer_path: "/m/t.json".into(),
            context: 4096,
        });
        s
    }

    fn proposal(id: u64, digest: &str) -> PendingAssistantProposal {
        PendingAssistantProposal {
            id,
            digest: digest.into(),
            diff: vec!["Priority (nice): 10 -> 12".into()],
            proposed: AssistantSettings {
                nice: 12,
                ..settings()
            },
            seconds_left: 300,
        }
    }

    fn state(
        current: AssistantSettings,
        pending: Option<PendingAssistantProposal>,
    ) -> TrustedWindowReply {
        TrustedWindowReply::AssistantSettingsState {
            current: Box::new(current),
            pending: pending.map(Box::new),
        }
    }

    fn open_with(
        current: AssistantSettings,
        pending: Option<PendingAssistantProposal>,
    ) -> AssistantPanel {
        let mut panel = AssistantPanel::default();
        assert!(panel.toggle());
        panel.on_reply(&state(current, pending));
        panel
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    // ---- stepping ----

    #[test]
    fn the_backend_cycles_only_through_backends_that_have_their_sections() {
        let mut s = AssistantSettings::default();
        assert!(
            !adjust(&mut s, Row::Backend, true),
            "nothing configured: only `none`"
        );
        s = settings();
        assert!(adjust(&mut s, Row::Backend, false));
        assert_eq!(s.backend, BackendKind::None);
        assert!(
            !adjust(&mut s, Row::Backend, false),
            "no wrap below the first"
        );
        assert!(adjust(&mut s, Row::Backend, true));
        assert_eq!(s.backend, BackendKind::Loopback);
        assert!(
            !adjust(&mut s, Row::Backend, true),
            "candle is unavailable without its section"
        );
        let mut both = with_candle(settings());
        both.backend = BackendKind::None;
        let seen: Vec<_> = (0..3)
            .map(|_| {
                adjust(&mut both, Row::Backend, true);
                both.backend
            })
            .collect();
        assert_eq!(
            seen,
            [
                BackendKind::Loopback,
                BackendKind::Candle,
                BackendKind::Both
            ]
        );
    }

    #[test]
    fn numeric_steppers_stop_at_the_validators_bounds_without_wrapping() {
        let mut s = settings();
        s.idle_timeout_secs = MIN_IDLE_TIMEOUT_SECS;
        assert!(!adjust(&mut s, Row::Idle, false));
        assert!(adjust(&mut s, Row::Idle, true));
        assert_eq!(s.idle_timeout_secs, MIN_IDLE_TIMEOUT_SECS + IDLE_STEP);
        s.idle_timeout_secs = MAX_IDLE_TIMEOUT_SECS;
        assert!(!adjust(&mut s, Row::Idle, true));

        s.nice = 0;
        assert!(!adjust(&mut s, Row::Nice, false));
        s.nice = MAX_NICE;
        assert!(!adjust(&mut s, Row::Nice, true));
        assert!(adjust(&mut s, Row::Nice, false));
        assert_eq!(s.nice, MAX_NICE - 1);
    }

    #[test]
    fn the_memory_ceiling_steps_through_a_minimum_and_then_no_limit() {
        let mut s = settings();
        s.max_resident_mb = None;
        assert!(!adjust(&mut s, Row::Ceiling, false), "already no limit");
        assert!(adjust(&mut s, Row::Ceiling, true));
        assert_eq!(s.max_resident_mb, Some(CEILING_FIRST));
        s.max_resident_mb = Some(MIN_RESIDENT_MB);
        assert!(adjust(&mut s, Row::Ceiling, false));
        assert_eq!(s.max_resident_mb, None, "below the minimum is no limit");
        s.max_resident_mb = Some(300);
        assert!(adjust(&mut s, Row::Ceiling, false));
        assert_eq!(
            s.max_resident_mb,
            Some(MIN_RESIDENT_MB),
            "never drops under the minimum by stepping"
        );
        s.max_resident_mb = Some(1000);
        assert!(adjust(&mut s, Row::Ceiling, false));
        assert_eq!(s.max_resident_mb, Some(744));
        s.max_resident_mb = Some(blueice_assistant_settings::MAX_RESIDENT_MB);
        assert!(!adjust(&mut s, Row::Ceiling, true));
    }

    #[test]
    fn the_candle_context_doubles_and_halves_within_its_bounds() {
        let mut s = with_candle(settings());
        assert!(adjust(&mut s, Row::CandleContext, true));
        assert_eq!(s.candle.as_ref().unwrap().context, 8192);
        s.candle.as_mut().unwrap().context = MIN_CANDLE_CONTEXT;
        assert!(!adjust(&mut s, Row::CandleContext, false));
        s.candle.as_mut().unwrap().context = MAX_CANDLE_CONTEXT;
        assert!(!adjust(&mut s, Row::CandleContext, true));
        let mut none = settings();
        assert!(
            !adjust(&mut none, Row::CandleContext, true),
            "no candle section, nothing to change"
        );
    }

    #[test]
    fn every_value_the_steppers_can_reach_is_valid_settings() {
        // Both backends configured, so every section is legitimately present.
        let mut s = with_candle(settings());
        s.backend = BackendKind::Both;
        for row in [Row::Idle, Row::Ceiling, Row::Nice, Row::CandleContext] {
            for up in [true, false] {
                for _ in 0..40 {
                    adjust(&mut s, row, up);
                    assert!(s.validate().is_ok(), "{row:?} up={up}: {:?}", s.validate());
                }
            }
        }
    }

    #[test]
    fn the_candle_row_appears_only_when_there_is_a_candle_section() {
        assert_eq!(rows(&settings()).len(), 4);
        assert_eq!(rows(&with_candle(settings())).len(), 5);
    }

    // ---- state machine ----

    #[test]
    fn opening_and_closing_are_window_gestures_and_state_comes_from_the_launcher() {
        let mut panel = AssistantPanel::default();
        assert!(!panel.is_open());
        assert!(panel.toggle());
        assert!(panel.is_open());
        panel.on_reply(&state(settings(), None));
        assert!(!panel.dirty());
        assert!(!panel.toggle());
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Escape), false, false),
            AssistantCommand::None
        );
    }

    #[test]
    fn approving_is_two_steps_and_carries_exactly_the_id_and_digest_on_screen() {
        let mut panel = open_with(settings(), Some(proposal(4, "dig-4")));
        assert_eq!(
            panel.key(&ch("a"), false, false),
            AssistantCommand::None,
            "first step only arms it"
        );
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::Send(TrustedWindowRequest::ApproveAssistantProposal {
                id: 4,
                digest: "dig-4".into()
            })
        );
        // The confirmation is consumed: a second Enter does nothing.
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::None
        );
    }

    #[test]
    fn escape_cancels_a_half_made_confirmation_without_closing_the_panel() {
        let mut panel = open_with(settings(), Some(proposal(4, "d")));
        panel.key(&ch("a"), false, false);
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Escape), false, false),
            AssistantCommand::None
        );
        assert!(panel.is_open());
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::None
        );
    }

    #[test]
    fn a_proposal_swapped_underneath_a_confirmation_cannot_inherit_the_approval() {
        let mut panel = open_with(settings(), Some(proposal(4, "old")));
        panel.key(&ch("a"), false, false);
        // The launcher reports a different proposal before the second step.
        panel.on_reply(&state(settings(), Some(proposal(5, "new"))));
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::None,
            "fresh state discards the confirmation"
        );
        // And even a confirmation forced past that is refused if the digest moved.
        panel.confirm = Confirm::Approve {
            id: 5,
            digest: "old".into(),
        };
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::None
        );
        assert!(panel.notice.as_deref().unwrap().contains("CHANGED"));
    }

    #[test]
    fn denying_is_one_step_and_needs_a_pending_proposal() {
        let mut panel = open_with(settings(), None);
        assert_eq!(panel.key(&ch("d"), false, false), AssistantCommand::None);
        let mut panel = open_with(settings(), Some(proposal(9, "d")));
        assert_eq!(
            panel.key(&ch("d"), false, false),
            AssistantCommand::Send(TrustedWindowRequest::DenyAssistantProposal { id: 9 })
        );
    }

    #[test]
    fn nothing_is_sent_while_a_request_is_in_flight_and_a_held_key_does_not_repeat_a_step() {
        let mut panel = open_with(settings(), Some(proposal(4, "d")));
        assert_eq!(
            panel.key(&ch("a"), true, false),
            AssistantCommand::None,
            "in flight: not even armed"
        );
        assert_eq!(panel.key(&ch("d"), true, false), AssistantCommand::None);
        assert_eq!(panel.key(&ch("r"), true, false), AssistantCommand::None);
        assert_eq!(
            panel.key(&ch("a"), false, true),
            AssistantCommand::None,
            "a repeat event is ignored"
        );
        panel.key(&ch("a"), false, false);
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, true),
            AssistantCommand::None,
            "a held Enter cannot confirm"
        );
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), true, false),
            AssistantCommand::None,
            "nor while in flight"
        );
    }

    #[test]
    fn edits_are_drafted_locally_and_applied_in_two_steps_with_the_whole_draft() {
        let mut panel = open_with(settings(), None);
        panel.key(&Key::Named(NamedKey::ArrowDown), false, false); // idle timeout
        panel.key(&Key::Named(NamedKey::ArrowRight), false, false); // +30
        assert!(panel.dirty());
        assert_eq!(panel.key(&ch("e"), false, false), AssistantCommand::None);
        let expected = AssistantSettings {
            idle_timeout_secs: 630,
            ..settings()
        };
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::Send(TrustedWindowRequest::EditAssistantSettings {
                settings: expected
            })
        );
    }

    #[test]
    fn applying_needs_an_actual_change_and_stepping_is_frozen_during_a_confirmation() {
        let mut panel = open_with(settings(), None);
        panel.key(&ch("e"), false, false);
        assert_eq!(
            panel.key(&Key::Named(NamedKey::Enter), false, false),
            AssistantCommand::None,
            "nothing to apply"
        );
        panel.key(&Key::Named(NamedKey::ArrowDown), false, false);
        panel.key(&Key::Named(NamedKey::ArrowRight), false, false);
        panel.key(&ch("e"), false, false);
        // With the confirmation showing, the draft cannot be changed under it.
        let before = panel.draft.clone();
        panel.key(&Key::Named(NamedKey::ArrowRight), false, false);
        assert_eq!(panel.draft, before);
    }

    #[test]
    fn a_rejection_is_shown_and_fresh_state_replaces_the_draft() {
        let mut panel = open_with(settings(), None);
        panel.key(&Key::Named(NamedKey::ArrowDown), false, false);
        panel.key(&Key::Named(NamedKey::ArrowRight), false, false);
        assert!(panel.dirty());
        panel.on_reply(&TrustedWindowReply::Rejected {
            reason: "nice must be 0 to 19".into(),
        });
        assert_eq!(panel.notice.as_deref(), Some("nice must be 0 to 19"));
        panel.on_reply(&state(settings(), None));
        assert!(
            !panel.dirty(),
            "what the launcher says is true replaces the unapplied draft"
        );
        assert!(panel.notice.is_none());
        // Replies about something else are ignored.
        panel.on_reply(&TrustedWindowReply::State {
            core_generation: 1,
            installed: None,
        });
        assert!(panel.notice.is_none());
    }

    #[test]
    fn refresh_asks_the_launcher_and_the_row_selection_stays_in_range() {
        let mut panel = open_with(with_candle(settings()), None);
        assert_eq!(
            panel.key(&ch("r"), false, false),
            AssistantCommand::Send(TrustedWindowRequest::InspectAssistantSettings)
        );
        for _ in 0..20 {
            panel.key(&Key::Named(NamedKey::ArrowDown), false, false);
        }
        assert_eq!(panel.row, 4);
        // State that drops the candle row pulls the selection back in range.
        panel.on_reply(&state(settings(), None));
        assert_eq!(panel.row, 3);
        for _ in 0..20 {
            panel.key(&Key::Named(NamedKey::ArrowUp), false, false);
        }
        assert_eq!(panel.row, 0);
    }

    // ---- clicks ----

    const W: u32 = 900;
    const H: u32 = 700;

    fn click_button(
        panel: &mut AssistantPanel,
        x: u32,
        y: u32,
        in_flight: bool,
    ) -> AssistantCommand {
        let rect = panel_rect(W, H).expect("a big enough window");
        let b = button(rect, x, y, 10);
        panel.click(b.x as f64 + 3.0, b.y as f64 + 3.0, W, H, in_flight)
    }

    #[test]
    fn the_buttons_do_what_the_keys_do() {
        let mut panel = open_with(settings(), Some(proposal(4, "d")));
        assert_eq!(
            click_button(&mut panel, 20, 439, false),
            AssistantCommand::None
        ); // APPROVE arms
        assert_eq!(
            click_button(&mut panel, 20, 439, false), // CONFIRM
            AssistantCommand::Send(TrustedWindowRequest::ApproveAssistantProposal {
                id: 4,
                digest: "d".into()
            })
        );
        assert_eq!(
            click_button(&mut panel, 160, 439, false),
            AssistantCommand::Send(TrustedWindowRequest::DenyAssistantProposal { id: 4 })
        );
        assert_eq!(
            click_button(&mut panel, 390, 439, false),
            AssistantCommand::Send(TrustedWindowRequest::InspectAssistantSettings)
        );
    }

    #[test]
    fn cancel_and_close_buttons_and_the_confirmation_layout() {
        let mut panel = open_with(settings(), Some(proposal(4, "d")));
        click_button(&mut panel, 20, 439, false); // arm approval
        assert_eq!(
            click_button(&mut panel, 230, 439, false),
            AssistantCommand::None
        ); // CANCEL
        assert_eq!(panel.confirm, Confirm::None);
        assert!(panel.is_open());
        assert_eq!(
            click_button(&mut panel, 550, 439, false),
            AssistantCommand::None
        ); // CLOSE
        assert!(!panel.is_open());
    }

    #[test]
    fn the_steppers_change_the_draft_and_select_their_row() {
        let mut panel = open_with(settings(), None);
        click_button(&mut panel, 520, ROW_TOP + 3 * ROW_STEP, false); // nice +
        assert_eq!(panel.row, 3);
        assert_eq!(panel.draft.as_ref().unwrap().nice, 11);
        click_button(&mut panel, 470, ROW_TOP + 3 * ROW_STEP, false); // nice -
        assert_eq!(panel.draft.as_ref().unwrap().nice, 10);
        // APPLY EDITS is only meaningful with a change.
        assert_eq!(
            click_button(&mut panel, 260, 439, false),
            AssistantCommand::None
        );
        assert_eq!(panel.confirm, Confirm::None);
        click_button(&mut panel, 520, ROW_TOP + 3 * ROW_STEP, false);
        click_button(&mut panel, 260, 439, false);
        assert_eq!(panel.confirm, Confirm::Apply);
    }

    #[test]
    fn clicks_do_nothing_while_a_request_is_in_flight_or_in_a_window_too_small() {
        let mut panel = open_with(settings(), Some(proposal(4, "d")));
        assert_eq!(
            click_button(&mut panel, 20, 439, true),
            AssistantCommand::None
        );
        assert_eq!(panel.confirm, Confirm::None, "not even armed");
        assert_eq!(
            click_button(&mut panel, 160, 439, true),
            AssistantCommand::None
        );
        assert_eq!(
            panel.click(10.0, 10.0, 200, 200, false),
            AssistantCommand::None
        );
    }

    // ---- drawing ----

    fn drawn(panel: &AssistantPanel, w: u32, h: u32, in_flight: bool) -> Vec<u32> {
        let mut pixels = vec![0x0000_0000u32; (w * h) as usize];
        panel.draw(&mut pixels, w, h, in_flight);
        pixels
    }

    #[test]
    fn a_closed_panel_draws_nothing_and_an_open_one_draws_inside_its_rect() {
        let closed = AssistantPanel::default();
        assert!(drawn(&closed, W, H, false).iter().all(|p| *p == 0));
        let panel = open_with(settings(), Some(proposal(4, "d")));
        let pixels = drawn(&panel, W, H, false);
        let rect = panel_rect(W, H).unwrap();
        let inside = |i: usize| {
            let (x, y) = ((i as u32) % W, (i as u32) / W);
            x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
        };
        assert!(pixels.iter().enumerate().any(|(i, p)| *p != 0 && inside(i)));
        assert!(
            pixels.iter().enumerate().all(|(i, p)| *p == 0 || inside(i)),
            "nothing outside the panel"
        );
    }

    #[test]
    fn every_screen_state_draws_without_panicking() {
        let mut panel = open_with(
            with_candle(settings()),
            Some(PendingAssistantProposal {
                diff: (0..20).map(|n| format!("Line {n}")).collect(),
                ..proposal(1, "d")
            }),
        );
        drawn(&panel, W, H, false);
        drawn(&panel, W, H, true);
        panel.key(&ch("a"), false, false);
        drawn(&panel, W, H, false);
        panel.key(&Key::Named(NamedKey::Escape), false, false);
        panel.on_reply(&TrustedWindowReply::Rejected {
            reason: "x".repeat(400),
        });
        drawn(&panel, W, H, false);
        let empty = open_with(AssistantSettings::default(), None);
        drawn(&empty, W, H, false);
        // Too small for the panel: a hint, not a crash.
        let hint = drawn(&panel, 300, 200, false);
        assert!(hint.iter().any(|p| *p != 0));
    }

    #[test]
    fn agent_influenced_text_cannot_spoof_the_chrome() {
        let line = safe_line("Model: ok\u{202e}evil\u{200b}\n\u{7}bell");
        assert!(
            !line.contains('\u{202e}')
                && !line.contains('\u{200b}')
                && !line.contains('\n')
                && !line.contains('\u{7}')
        );
        assert!(line.chars().count() <= LINE_CHARS);
        assert_eq!(safe_line(&"a".repeat(500)).chars().count(), LINE_CHARS);
    }
}
