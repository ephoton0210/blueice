// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One host task turn arbitrates timers and document browser work. Rendering
//! remains a separate entry; Core can offer it after each selected task.

use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/host_task_source_boundaries.rs"]
mod boundary_tests;

const MAX_DOCUMENT_TASKS_PER_TURN: usize = 64;

/// Result of a bounded page task turn, without callbacks or source text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlueJsTaskTurn {
    /// Task deliveries, each followed by its microtask checkpoint.
    pub tasks_run: u32,
    /// Relative host milliseconds. Zero means work is immediately runnable;
    /// `None` means no task wake is needed, including while debugger-paused.
    pub next_wake_in_ms: Option<u64>,
}

// Each logical source has one document-wide FIFO. The host chooses among
// runnable sources in round-robin order; storage location and realm do not
// create a separate source. Future network/UI ingress must preserve this
// source association and the one-task Core boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskSource {
    JavaScriptEngine,
    FileReading,
    Timer,
    DomManipulation,
    Networking,
    UserInteraction,
}

const TASK_SOURCES: [TaskSource; 6] = [
    TaskSource::JavaScriptEngine,
    TaskSource::FileReading,
    TaskSource::Timer,
    TaskSource::DomManipulation,
    TaskSource::Networking,
    TaskSource::UserInteraction,
];

#[derive(Clone, Copy)]
enum Delivery {
    Finalization,
    Timer(u32),
    Browser(usize),
}

struct Candidate {
    order: (u64, u64),
    delivery: Delivery,
}

#[derive(Default)]
struct TimerSnapshot {
    callbacks: Vec<u32>,
    aborts: HashSet<u64>,
}

impl Vm {
    pub(super) fn next_host_task_serial(&mut self) -> Result<u64, RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            return scope.next_task_serial();
        }
        let serial = self.next_standalone_task_serial;
        self.next_standalone_task_serial =
            serial.checked_add(1).ok_or(RuntimeError::SchedulingLimit)?;
        Ok(serial)
    }

    fn document_browser_wake(&mut self) -> Option<u64> {
        let mut wake = self.next_own_browser_task_wake();
        if !self.host_finalization_tasks.is_empty() {
            wake = Some(0);
        }
        for record in self.host_shadow_realms() {
            let Some(realm) = record.upgrade() else {
                continue;
            };
            let mut vm = realm.borrow_mut();
            if vm.ensure_no_debugger_continuation().is_err() {
                continue;
            }
            let now = vm.sample_host_timer_clock();
            let child = if vm.host_finalization_tasks.is_empty() {
                vm.next_own_browser_task_wake()
            } else {
                Some(0)
            };
            wake = earliest_wake(wake, child);
            wake = earliest_wake(
                wake,
                vm.next_host_timer_due().map(|due| due.saturating_sub(now)),
            );
        }
        wake
    }

    fn checkpoint_document_task_entry(&mut self) -> Result<(), RuntimeError> {
        self.run_host_microtask_checkpoint()?;
        for record in self.host_shadow_realms() {
            let Some(realm) = record.upgrade() else {
                continue;
            };
            let mut vm = realm.borrow_mut();
            if vm.ensure_no_debugger_continuation().is_ok() {
                vm.sample_host_timer_clock();
                vm.run_host_microtask_checkpoint()?;
            }
        }
        Ok(())
    }

    pub(crate) fn next_host_task_wake(&mut self, now_ms: u64) -> Result<Option<u64>, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(None);
        }
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        let now = self.sample_host_timer_clock();
        let timers = self
            .next_host_timer_due()
            .map(|due| due.saturating_sub(now));
        Ok(earliest_wake(self.document_browser_wake(), timers))
    }

    fn snapshot_task_timers(&mut self, include_callbacks: bool) -> TimerSnapshot {
        let now = self.sample_host_timer_clock();
        TimerSnapshot {
            callbacks: if include_callbacks {
                self.snapshot_due_host_timers(now)
            } else {
                Vec::new()
            },
            aborts: self
                .browser_tasks
                .iter()
                .filter_map(|task| {
                    if task.source() == TaskSource::Timer && self.browser_task_wake(task) == 0 {
                        Some(task.order().1)
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    fn own_task_candidate(
        &self,
        source: TaskSource,
        timers: Option<&TimerSnapshot>,
    ) -> Option<Candidate> {
        let mut candidate = if source == TaskSource::JavaScriptEngine {
            self.host_finalization_tasks.front().map(|task| Candidate {
                order: (0, task.serial),
                delivery: Delivery::Finalization,
            })
        } else {
            None
        };
        for (index, task) in self.browser_tasks.iter().enumerate() {
            if task.source() != source || self.browser_task_wake(task) != 0 {
                continue;
            }
            let order = task.order();
            if source == TaskSource::Timer
                && !timers.is_some_and(|snapshot| snapshot.aborts.contains(&order.1))
            {
                continue;
            }
            if candidate
                .as_ref()
                .is_none_or(|current| order < current.order)
            {
                candidate = Some(Candidate {
                    order,
                    delivery: Delivery::Browser(index),
                });
            }
        }
        if source == TaskSource::Timer {
            if let Some(snapshot) = timers {
                for timer in &self.host_timers {
                    if snapshot.callbacks.contains(&timer.id) {
                        let order = (timer.due_ms, timer.serial);
                        if candidate
                            .as_ref()
                            .is_none_or(|current| order < current.order)
                        {
                            candidate = Some(Candidate {
                                order,
                                delivery: Delivery::Timer(timer.id),
                            });
                        }
                    }
                }
            }
        }
        candidate
    }

    fn deliver_host_task(&mut self, delivery: Delivery) -> Result<(), RuntimeError> {
        match delivery {
            Delivery::Finalization => self.run_next_host_finalization_task(),
            Delivery::Timer(id) => self.run_host_timer_entry(id, &mut BlueJsTimerRun::default()),
            Delivery::Browser(index) => self.run_own_browser_task(index),
        }
    }

    pub(crate) fn run_host_task_turn(
        &mut self,
        now_ms: u64,
        max_tasks: usize,
    ) -> Result<BlueJsTaskTurn, RuntimeError> {
        self.run_host_task_turn_inner(now_ms, max_tasks, true)
    }

    pub(super) fn run_host_task_turn_inner(
        &mut self,
        now_ms: u64,
        max_tasks: usize,
        include_timer_callbacks: bool,
    ) -> Result<BlueJsTaskTurn, RuntimeError> {
        self.check_scheduling_failure()?;
        if self.ensure_no_debugger_continuation().is_err() {
            return Ok(BlueJsTaskTurn::default());
        }
        self.host_timer_clock_ms = self.host_timer_clock_ms.max(now_ms);
        self.sample_host_timer_clock();
        self.checkpoint_document_task_entry()?;
        let mut snapshots = HashMap::new();
        snapshots.insert(
            self.object_prototype.heap,
            self.snapshot_task_timers(include_timer_callbacks),
        );
        for record in self.host_shadow_realms() {
            let Some(realm) = record.upgrade() else {
                continue;
            };
            let mut vm = realm.borrow_mut();
            if vm.ensure_no_debugger_continuation().is_ok() {
                let heap = vm.object_prototype.heap;
                snapshots.insert(heap, vm.snapshot_task_timers(include_timer_callbacks));
            }
        }
        let mut turn = BlueJsTaskTurn::default();
        while (turn.tasks_run as usize) < max_tasks.min(MAX_DOCUMENT_TASKS_PER_TURN) {
            let realms = self.host_shadow_realms();
            // A callback can create a realm or queue work through a wrapped
            // function. Finish those child jobs before selecting another task.
            for record in &realms {
                let Some(realm) = record.upgrade() else {
                    continue;
                };
                let mut vm = realm.borrow_mut();
                if vm.ensure_no_debugger_continuation().is_ok() {
                    vm.sample_host_timer_clock();
                    vm.run_host_microtask_checkpoint()?;
                }
            }
            let mut selected = None;
            for offset in 0..TASK_SOURCES.len() {
                let source_index = (self.next_host_task_source + offset) % TASK_SOURCES.len();
                let source = TASK_SOURCES[source_index];
                let mut candidate = self
                    .own_task_candidate(source, snapshots.get(&self.object_prototype.heap))
                    .map(|task| (0, task));
                for (index, record) in realms.iter().enumerate() {
                    let Some(realm) = record.upgrade() else {
                        continue;
                    };
                    let vm = realm.borrow();
                    if vm.ensure_no_debugger_continuation().is_err() {
                        continue;
                    }
                    if let Some(task) =
                        vm.own_task_candidate(source, snapshots.get(&vm.object_prototype.heap))
                    {
                        if candidate
                            .as_ref()
                            .is_none_or(|(_, current)| task.order < current.order)
                        {
                            candidate = Some((index + 1, task));
                        }
                    }
                }
                if let Some((owner, task)) = candidate {
                    selected = Some((source_index, owner, task.delivery));
                    break;
                }
            }
            let Some((source, owner, delivery)) = selected else {
                break;
            };
            // Timers keep their turn snapshot even after an interval rearms.
            let heap = if owner == 0 {
                self.object_prototype.heap
            } else {
                realms[owner - 1]
                    .upgrade()
                    .expect("selected task owns its realm")
                    .borrow()
                    .object_prototype
                    .heap
            };
            if let Delivery::Timer(id) = delivery {
                if let Some(snapshot) = snapshots.get_mut(&heap) {
                    snapshot.callbacks.retain(|timer| *timer != id);
                }
            }
            self.next_host_task_source = (source + 1) % TASK_SOURCES.len();
            if owner == 0 {
                self.deliver_host_task(delivery)?;
            } else {
                realms[owner - 1]
                    .upgrade()
                    .expect("selected task owns its realm")
                    .borrow_mut()
                    .deliver_host_task(delivery)?;
            }
            turn.tasks_run += 1;
        }
        turn.next_wake_in_ms = self.next_host_task_wake(now_ms)?;
        Ok(turn)
    }
}

fn earliest_wake(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}
