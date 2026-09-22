// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The small scheduling kernel shared by BlueJS's host integrations.
//!
//! A macrotask always checkpoints all queued microtasks before the next
//! macrotask. Rendering is deliberately *not* another queued macrotask:
//! [`EventLoop::run_render_pass`] first runs document-order scheduled script,
//! then checkpoints microtasks, then invokes layout/paint. This mirrors the
//! two entry points selected in Phase 13's event-loop design.

use std::collections::VecDeque;

type Task<Host> = Box<dyn FnOnce(&mut Host, &mut EventLoop<Host>) + Send>;

/// One host-owned event loop. `Host` is normally a tab's script realm plus its
/// DOM adapter; keeping it generic lets the core own page state while BlueJS
/// owns no raw DOM pointers across its process boundary.
pub struct EventLoop<Host> {
    macrotasks: VecDeque<Task<Host>>,
    microtasks: VecDeque<Task<Host>>,
}

impl<Host> Default for EventLoop<Host> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Host> EventLoop<Host> {
    pub fn new() -> Self {
        Self {
            macrotasks: VecDeque::new(),
            microtasks: VecDeque::new(),
        }
    }

    pub fn queue_macrotask(&mut self, task: impl FnOnce(&mut Host, &mut Self) + Send + 'static) {
        self.macrotasks.push_back(Box::new(task));
    }

    pub fn queue_microtask(&mut self, task: impl FnOnce(&mut Host, &mut Self) + Send + 'static) {
        self.microtasks.push_back(Box::new(task));
    }

    pub fn pending_macrotasks(&self) -> usize {
        self.macrotasks.len()
    }

    pub fn pending_microtasks(&self) -> usize {
        self.microtasks.len()
    }

    /// Runs one macrotask, then drains every microtask that task (or another
    /// microtask) enqueues. Returns `false` only if there was no macrotask.
    pub fn run_next_macrotask(&mut self, host: &mut Host) -> bool {
        let Some(task) = self.macrotasks.pop_front() else {
            return false;
        };
        task(host, self);
        self.checkpoint_microtasks(host);
        true
    }

    /// Drains the microtask queue to quiescence. A microtask enqueuing another
    /// microtask runs it in this same checkpoint, never after a later timer.
    pub fn checkpoint_microtasks(&mut self, host: &mut Host) {
        while let Some(task) = self.microtasks.pop_front() {
            task(host, self);
        }
    }

    /// Phase 3's render entry point: scheduled parser/render script runs in
    /// its own synchronous phase, a microtask checkpoint follows it, and only
    /// then may the host cascade/layout/paint. It intentionally does not run
    /// unrelated queued macrotasks ahead of the frame.
    pub fn run_render_pass(
        &mut self,
        host: &mut Host,
        scheduled_script: impl FnOnce(&mut Host, &mut Self),
        render: impl FnOnce(&mut Host),
    ) {
        scheduled_script(host, self);
        self.checkpoint_microtasks(host);
        render(host);
        self.checkpoint_microtasks(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_macrotask_drains_its_microtasks_before_the_next_macrotask() {
        let mut events = Vec::new();
        let mut loop_ = EventLoop::new();
        loop_.queue_macrotask(|events: &mut Vec<&'static str>, loop_| {
            events.push("timer-1");
            loop_.queue_microtask(|events, _| events.push("microtask-1"));
            loop_.queue_microtask(|events, loop_| {
                events.push("microtask-2");
                loop_.queue_microtask(|events, _| events.push("microtask-3"));
            });
        });
        loop_.queue_macrotask(|events, _| events.push("timer-2"));

        assert!(loop_.run_next_macrotask(&mut events));
        assert!(loop_.run_next_macrotask(&mut events));

        assert_eq!(
            events,
            [
                "timer-1",
                "microtask-1",
                "microtask-2",
                "microtask-3",
                "timer-2"
            ]
        );
    }

    #[test]
    fn render_pass_runs_scheduled_script_and_its_microtasks_before_paint() {
        let mut events = Vec::new();
        let mut loop_ = EventLoop::new();
        loop_.queue_macrotask(|events: &mut Vec<&'static str>, _| events.push("unrelated-timer"));

        loop_.run_render_pass(
            &mut events,
            |events, loop_| {
                events.push("script");
                loop_.queue_microtask(|events, _| events.push("script-microtask"));
            },
            |events| events.push("paint"),
        );

        assert_eq!(events, ["script", "script-microtask", "paint"]);
        assert_eq!(
            loop_.pending_macrotasks(),
            1,
            "rendering must not consume unrelated timers"
        );
    }
}
