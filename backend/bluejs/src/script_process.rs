// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The always-resident BlueJS process loop.
//!
//! One process owns a distinct [`crate::Vm`] for each core tab. All VMs share
//! one long-lived script socket, but each host clone fixes the tab ID carried
//! on every DOM request, so a closure in one realm cannot accidentally mutate
//! another tab.

use crate::{ScriptDomClient, Vm};
use blueice_ipc::script::{ScriptCommand, ScriptTurnOutcome};
use std::collections::HashMap;
use std::io::{Read, Write};

pub fn run_script_process<S: Read + Write + std::fmt::Debug + 'static>(
    stream: S,
) -> Result<(), String> {
    let mut control = ScriptDomClient::handshake(stream, 0).map_err(|error| error.to_string())?;
    let mut realms = HashMap::<u64, Vm>::new();

    loop {
        match control.read_command()? {
            ScriptCommand::ResetRealm { tab_id } => {
                realms.remove(&tab_id);
                control.complete(tab_id, None, ScriptTurnOutcome::default())?;
            }
            ScriptCommand::Execute { tab_id, source } => {
                let realm = realms.entry(tab_id).or_insert_with(|| {
                    let mut vm = Vm::new();
                    vm.set_dom_host(Box::new(control.for_tab(tab_id)))
                        .expect("a fresh BlueJS realm can install its script host");
                    vm
                });
                let result = realm.evaluate(&source);
                control.complete(
                    tab_id,
                    result.as_ref().err().map(ToString::to_string),
                    ScriptTurnOutcome::default(),
                )?;
            }
            ScriptCommand::DispatchEvent {
                tab_id,
                node,
                event_type,
            } => {
                let Some(realm) = realms.get_mut(&tab_id) else {
                    control.complete(tab_id, None, ScriptTurnOutcome::default())?;
                    continue;
                };
                let ran_event = realm.has_event_listeners(node, &event_type);
                let result = realm.dispatch_event(node, &event_type);
                let outcome = ScriptTurnOutcome {
                    default_prevented: result.as_ref().copied().unwrap_or(false),
                    ran_timers: false,
                    ran_event,
                };
                control.complete(tab_id, result.err().map(|error| error.to_string()), outcome)?;
            }
            ScriptCommand::RunDueTimers { tab_id } => {
                let result = match realms.get_mut(&tab_id) {
                    Some(realm) => realm.run_due_timers(),
                    // A tab can receive UI messages before it has ever loaded
                    // a script-bearing document; that is intentionally a
                    // quiet no-op rather than a process-level protocol error.
                    None => Ok(false),
                };
                let outcome = ScriptTurnOutcome {
                    default_prevented: false,
                    ran_timers: result.as_ref().copied().unwrap_or(false),
                    ran_event: false,
                };
                control.complete(tab_id, result.err().map(|error| error.to_string()), outcome)?;
            }
            ScriptCommand::Shutdown => return Ok(()),
        }
    }
}
