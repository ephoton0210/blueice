// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `blueice-downloads`: the isolated process that owns downloads
//! (`phase-10-download-manager/PLAN.md`). The transfer mechanics live in
//! `blueice_net::download`; this crate adds what a *process* needs on top:
//! the queue and lifecycle ([`manager`]), where files may land
//! ([`policy`]), persistence across restarts ([`store`]), the Unix-socket
//! protocol server ([`server`]), and the command line ([`cli`]).

pub mod cli;
pub mod manager;
pub mod policy;
pub mod server;
pub mod store;
