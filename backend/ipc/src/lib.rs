// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Control-plane IPC protocol shared by `core`'s three kinds of
//! out-of-process clients: `extension`, `frontend`, and the Phase 5
//! AI-facing API (`BROWSER_CORE_PLAN.md` §1, Process architecture).
//!
//! Not yet designed. Needs a wire format usable from non-Rust frontends
//! (WinUI 3/C#, SwiftUI/Swift, PySide6/Python) -- likely a schema-driven
//! protocol (e.g. protobuf or Cap'n Proto) rather than a Rust-only serde
//! format, though that choice isn't made yet either. This crate is
//! reserved as a workspace member ahead of that design work so its
//! eventual dependents (`extension`, Phase 4's frontend boundary, Phase
//! 5's AI-facing API) don't need restructuring to adopt it.
