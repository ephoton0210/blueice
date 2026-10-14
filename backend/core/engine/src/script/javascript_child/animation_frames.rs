// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl<C: PageHostClient> OutOfProcessJavaScriptPageExecutor<C> {
    pub(super) fn update_scheduling_reply(
        &mut self,
        tab: TabId,
        started: Instant,
        next_timer: Option<u64>,
        frame_pending: bool,
    ) -> io::Result<()> {
        if next_timer.is_some_and(|delay| delay > i32::MAX as u64) {
            return Err(io::Error::other("Page host timer delay invalid"));
        }
        if let Some(delay) = next_timer {
            self.timer_wakes
                .insert(tab, started + Duration::from_millis(delay));
        } else {
            self.timer_wakes.remove(&tab);
        }
        if frame_pending && self.child.animation_frames_available() {
            // Task polling cannot keep postponing an already requested frame.
            self.animation_frame_wakes
                .entry(tab)
                .or_insert(started + ANIMATION_FRAME_INTERVAL);
        } else {
            self.animation_frame_wakes.remove(&tab);
        }
        Ok(())
    }

    pub(super) fn render_opportunities(
        &mut self,
        tabs: &mut TabManager,
        script_requests: Option<&ScriptRequestReceiver>,
    ) -> io::Result<()> {
        if !self.child.animation_frames_available() {
            return Ok(());
        }
        let Some(script_requests) = script_requests else {
            return Ok(());
        };
        let now = Instant::now();
        let due = self
            .animation_frame_wakes
            .iter()
            .filter(|(_, due)| **due <= now)
            .map(|(tab, _)| *tab)
            .collect::<Vec<_>>();
        for tab in due {
            self.animation_frame_wakes.remove(&tab);
            let Some(identity) = tabs.get(tab).and_then(live_page_identity) else {
                continue;
            };
            if self.live_documents.get(&tab) != Some(&identity)
                || self
                    .realm_stats
                    .get(&tab)
                    .is_none_or(|stats| stats.document_generation != identity.document_generation)
            {
                continue;
            }
            if self
                .render_one_frame(tabs, script_requests, tab, identity.document_generation)
                .is_err()
            {
                // Same containment as task turns: revoke the failed realm,
                // retain its admission identity so scripts are not replayed.
                self.close_page(tab);
                self.live_documents.insert(tab, identity);
            }
        }
        Ok(())
    }

    fn render_one_frame(
        &mut self,
        tabs: &mut TabManager,
        script_requests: &ScriptRequestReceiver,
        tab: TabId,
        generation: u64,
    ) -> io::Result<()> {
        let target = ScriptDocumentTarget {
            tab_id: tab.as_u64(),
            document_generation: generation,
        };
        let mut remaining = MAX_NESTED_SCRIPT_REQUESTS_PER_WAIT;
        let mut pump = || {
            pump_script_requests_during_child_wait(script_requests, tabs, target, &mut remaining)
        };
        let started = Instant::now();
        match self.child.run_animation_frame_with_script_pump(
            target.tab_id,
            target.document_generation,
            &mut pump,
        )? {
            PageHostReply::AnimationFrameRendered {
                tab_id,
                document_generation,
                next_wake_in_ms,
                animation_frame_pending,
            } if tab_id == target.tab_id && document_generation == target.document_generation => {
                self.update_scheduling_reply(tab, started, next_wake_in_ms, animation_frame_pending)
            }
            _ => Err(io::Error::other("Page host frame response invalid")),
        }
    }
}
