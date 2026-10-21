// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rejection notification snapshots are prepared while their task owns roots.

use super::browser_tasks::BrowserTask;
use super::host_scheduling::SharedSchedulingCharge;
use super::*;
use crate::heap::DragEventState;

const ATTRIBUTE_COUNT: usize = 8;
type Attribute = (&'static str, Value);

pub(super) struct PreparedRejectionEntry {
    pub(super) promise: ObjectId,
    pub(super) reason: Value,
    pub(super) source: Option<crate::diagnostics::DiagnosticSourceLocation>,
    pub(super) state: Option<DragEventState>,
    pub(super) state_charge: Option<SharedSchedulingCharge>,
    pub(super) source_charge: Option<SharedSchedulingCharge>,
    // Payloads are destroyed before their leases, including on refusal.
    _reason_charge: Option<SharedSchedulingCharge>,
}
impl PreparedRejectionEntry {
    fn state_bytes(&self) -> usize {
        self.state.as_ref().map_or(0, |state| {
            state.attributes.iter().fold(
                host_scheduling::slots(&state.attributes),
                |bytes, (_, value)| {
                    bytes.saturating_add(host_scheduling::value_payload_bytes(value))
                },
            )
        })
    }
    fn source_bytes(&self) -> usize {
        self.source
            .as_ref()
            .map_or(0, |source| source.filename.capacity())
    }
    fn retained_bytes(&self) -> usize {
        self.state_bytes()
            .saturating_add(self.source_bytes())
            .saturating_add(host_scheduling::value_payload_bytes(&self.reason))
    }
}
pub(super) struct PreparedRejectionBatch {
    pub(super) entries: Vec<PreparedRejectionEntry>,
    // Only vector backing remains here after payload ownership is split off.
    charge: Option<SharedSchedulingCharge>,
}
impl PreparedRejectionBatch {
    #[cfg(any(test, coverage))]
    fn retained_bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(host_scheduling::slots(&self.entries), |bytes, entry| {
                bytes.saturating_add(entry.retained_bytes())
            })
    }
}

impl Vm {
    pub(super) fn prepare_rejection_batch(
        &self,
        index: usize,
        owner: Option<&SharedSchedulingCharge>,
    ) -> Result<PreparedRejectionBatch, RuntimeError> {
        let BrowserTask::PromiseRejections {
            entries, handled, ..
        } = &self.browser_tasks[index]
        else {
            unreachable!("rejection task selected")
        };
        let event_name = if *handled {
            "rejectionhandled"
        } else {
            "unhandledrejection"
        };
        let forecast = |entry: &host_rejections::PendingRejection| {
            let record = &self.promises[&entry.promise];
            match &record.status {
                PromiseStatus::Rejected(reason) if *handled || !record.handled => ATTRIBUTE_COUNT
                    .saturating_mul(std::mem::size_of::<Attribute>())
                    .saturating_add(event_name.len().saturating_mul(2))
                    .saturating_add(host_scheduling::value_payload_bytes(reason).saturating_mul(2))
                    .saturating_add(
                        record
                            .rejection_origin
                            .source
                            .as_ref()
                            .map_or(0, |source| source.filename.len()),
                    ),
                _ => 0,
            }
        };
        let mut remaining = entries
            .iter()
            .fold(0usize, |bytes, entry| bytes.saturating_add(forecast(entry)));
        let minimum = entries
            .len()
            .saturating_mul(std::mem::size_of::<PreparedRejectionEntry>())
            .saturating_add(remaining);
        let fail = || owner.map_or(RuntimeError::SchedulingLimit, SharedSchedulingCharge::fail);
        let charge = owner.map(|owner| owner.reserve_peer(minimum)).transpose()?;
        let mut batch = PreparedRejectionBatch {
            entries: Vec::new(),
            charge,
        };
        batch
            .entries
            .try_reserve_exact(entries.len())
            .map_err(|_| fail())?;
        let slots = host_scheduling::slots(&batch.entries);
        if let Some(charge) = &mut batch.charge {
            charge.resize(slots.saturating_add(remaining))?;
        }
        let mut copied = 0usize;
        for entry in entries {
            let expected = forecast(entry);
            let mut prepared = PreparedRejectionEntry {
                promise: entry.promise,
                reason: Value::Undefined,
                source: None,
                state: None,
                state_charge: None,
                source_charge: None,
                _reason_charge: None,
            };
            if expected != 0 {
                let record = &self.promises[&entry.promise];
                let PromiseStatus::Rejected(reason) = &record.status else {
                    unreachable!()
                };
                let mut state = DragEventState::new(7);
                state
                    .attributes
                    .try_reserve_exact(ATTRIBUTE_COUNT)
                    .map_err(|_| fail())?;
                let reserved_slots = host_scheduling::slots(&state.attributes);
                let reserved_extra = reserved_slots
                    .saturating_sub(ATTRIBUTE_COUNT * std::mem::size_of::<Attribute>());
                if let Some(charge) = &mut batch.charge {
                    charge.resize(
                        slots
                            .saturating_add(copied)
                            .saturating_add(remaining)
                            .saturating_add(reserved_extra),
                    )?;
                }
                self.initialize_web_event_state(&mut state, event_name.into());
                state.cancelable = !handled;
                state.set("promise", Value::Object(entry.promise));
                state.set("reason", reason.clone());
                assert_eq!(host_scheduling::slots(&state.attributes), reserved_slots);
                prepared.state = Some(state);
                prepared.reason = reason.clone();
                if let Some(source) = &record.rejection_origin.source {
                    let mut filename = String::new();
                    filename
                        .try_reserve_exact(source.filename.len())
                        .map_err(|_| fail())?;
                    let source_extra = filename.capacity().saturating_sub(source.filename.len());
                    if let Some(charge) = &mut batch.charge {
                        charge.resize(
                            slots
                                .saturating_add(copied)
                                .saturating_add(remaining)
                                .saturating_add(reserved_extra)
                                .saturating_add(source_extra),
                        )?;
                    }
                    filename.push_str(&source.filename);
                    prepared.source = Some(crate::diagnostics::DiagnosticSourceLocation {
                        filename,
                        line: source.line,
                        column: source.column,
                    });
                }
            }
            remaining -= expected;
            copied = copied.saturating_add(prepared.retained_bytes());
            if let Some(charge) = &mut batch.charge {
                charge.resize(slots.saturating_add(copied).saturating_add(remaining))?;
            }
            batch.entries.push(prepared);
        }
        if let Some(charge) = &mut batch.charge {
            for entry in &mut batch.entries {
                entry.state_charge = Some(charge.split_off(entry.state_bytes()));
                entry.source_charge = Some(charge.split_off(entry.source_bytes()));
                entry._reason_charge =
                    Some(charge.split_off(host_scheduling::value_payload_bytes(&entry.reason)));
            }
        }
        Ok(batch)
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/rejection_batch_entry.rs"]
mod ownership_tests;
