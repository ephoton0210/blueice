// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Connection authentication for
/// [`handle_extension_connection_with_actions_and_authentication`].
///
/// The standalone protocol server uses [`Self::unauthenticated`]. Core's
/// host-spawned mode uses [`Self::required`] and can attach a one-shot
/// readiness sender that fires only after the first valid handshake.
pub struct ExtensionConnectionAuthentication<'a> {
    pub(super) expected: Option<&'a str>,
    pub(super) authenticated_ready: Option<mpsc::Sender<()>>,
    pub(super) runtime_start: Option<Arc<Mutex<mpsc::Receiver<()>>>>,
    pub(super) runtime_events: Option<Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>>,
}

/// Core-owned delegates for one authenticated extension connection. Bundling
/// the protocol effects keeps the long-lived connection handler's
/// authority surface explicit without growing its public argument list every
/// time a new, independently reviewed operation is added.
pub struct ExtensionActionDelegates<R, W, N, B, C> {
    pub(super) read_dom: R,
    pub(super) read_ephemeral_dom: Box<dyn FnMut(u64, String) -> Result<String, String> + Send>,
    pub(super) write_dom: W,
    pub(super) register_network_intercept: N,
    pub(super) register_network_block_url: B,
    pub(super) register_network_block_host:
        Box<dyn FnMut(String, u64) -> Result<(), String> + Send>,
    pub(super) register_network_block_path_prefix:
        Box<dyn FnMut(String, String, u64) -> Result<(), String> + Send>,
    pub(super) register_network_redirect_url:
        Box<dyn FnMut(String, String, u64) -> Result<(), String> + Send>,
    pub(super) clear_network_block_urls: C,
    pub(super) observe_network:
        Box<dyn FnMut(u64) -> Result<Option<NetworkResponseInfo>, String> + Send>,
    pub(super) observe_network_trace:
        Box<dyn FnMut(u64) -> Result<Option<NetworkTraceInfo>, String> + Send>,
    pub(super) set_toolbar_button: Box<dyn FnMut(String, u64) -> Result<(), String> + Send>,
    pub(super) clear_toolbar_button: Box<dyn FnMut() -> Result<(), String> + Send>,
    #[allow(clippy::type_complexity)]
    pub(super) show_popup: Box<dyn FnMut(u64, String, String, u64) -> Result<(), String> + Send>,
    #[allow(clippy::type_complexity)]
    pub(super) show_popup_action:
        Box<dyn FnMut(u64, String, String, String, u64) -> Result<(), String> + Send>,
    pub(super) clear_popup: Box<dyn FnMut() -> Result<(), String> + Send>,
    pub(super) storage: ExtensionStorage,
}

impl<R, W, N, B, C> ExtensionActionDelegates<R, W, N, B, C> {
    /// Creates the complete delegate bundle for one connection. Each closure
    /// is still invoked only after the handler's normal capability, version,
    /// and (where required) gatekeeper checks.
    pub fn new(
        read_dom: R,
        write_dom: W,
        register_network_intercept: N,
        register_network_block_url: B,
        clear_network_block_urls: C,
    ) -> Self {
        Self {
            read_dom,
            read_ephemeral_dom: Box::new(|_, _| {
                Err("runtime-ephemeral dom:read needs a core-owned one-shot reader".to_string())
            }),
            write_dom,
            register_network_intercept,
            register_network_block_url,
            register_network_block_host: Box::new(|_, _| {
                Err("network:intercept v4 needs a core-backed host rule store".to_string())
            }),
            register_network_block_path_prefix: Box::new(|_, _, _| {
                Err("network:intercept v5 needs a core-backed path-prefix rule store".to_string())
            }),
            register_network_redirect_url: Box::new(|_, _, _| {
                Err("network:intercept v6 needs a core-backed redirect rule store".to_string())
            }),
            clear_network_block_urls,
            observe_network: Box::new(|_| {
                Err("network:observe needs a core-backed response reader".to_string())
            }),
            observe_network_trace: Box::new(|_| {
                Err("network:observe v2 needs a core-backed trace reader".to_string())
            }),
            set_toolbar_button: Box::new(|_, _| {
                Err("ui:inject needs a core-backed native toolbar".to_string())
            }),
            clear_toolbar_button: Box::new(|| {
                Err("ui:inject needs a core-backed native toolbar".to_string())
            }),
            show_popup: Box::new(|_, _, _, _| {
                Err("ui:inject version 2 needs a core-backed native popup".to_string())
            }),
            show_popup_action: Box::new(|_, _, _, _, _| {
                Err("ui:inject version 3 needs a core-backed popup action".to_string())
            }),
            clear_popup: Box::new(|| {
                Err("ui:inject version 2 needs a core-backed native popup".to_string())
            }),
            storage: ExtensionStorage::default(),
        }
    }

    /// Replaces the default isolated bucket handle with the core-owned handle
    /// shared by every connection for one extension service. This preserves
    /// per-identity state across reconnects without giving the guest a path,
    /// process handle, or mutable reference to the map.
    pub fn with_storage(mut self, storage: ExtensionStorage) -> Self {
        self.storage = storage;
        self
    }

    /// Binds a one-shot `dom:read` to the core session's final document-epoch
    /// and ticket check. The ordinary read delegate cannot authorize it.
    pub fn with_ephemeral_dom_reader(
        mut self,
        reader: impl FnMut(u64, String) -> Result<String, String> + Send + 'static,
    ) -> Self {
        self.read_ephemeral_dom = Box::new(reader);
        self
    }

    /// Binds read-only response observation to core's live tab state.
    pub fn with_network_observer(
        mut self,
        observer: impl FnMut(u64) -> Result<Option<NetworkResponseInfo>, String> + Send + 'static,
    ) -> Self {
        self.observe_network = Box::new(observer);
        self
    }

    /// Binds v2 request/redirect observation to committed core-owned state.
    pub fn with_network_trace_observer(
        mut self,
        observer: impl FnMut(u64) -> Result<Option<NetworkTraceInfo>, String> + Send + 'static,
    ) -> Self {
        self.observe_network_trace = Box::new(observer);
        self
    }

    /// Binds v4 declarative host blocking to core's connection-owned rules.
    pub fn with_network_block_host(
        mut self,
        blocker: impl FnMut(String, u64) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_block_host = Box::new(blocker);
        self
    }

    /// Binds v5 literal host/path-prefix blocking to core's owned rule set.
    pub fn with_network_block_path_prefix(
        mut self,
        blocker: impl FnMut(String, String, u64) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_block_path_prefix = Box::new(blocker);
        self
    }

    /// Binds v6 exact same-origin navigation rewrites to core's rule store.
    pub fn with_network_redirect_url(
        mut self,
        redirector: impl FnMut(String, String, u64) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.register_network_redirect_url = Box::new(redirector);
        self
    }

    pub fn with_toolbar_button(
        mut self,
        setter: impl FnMut(String, u64) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.set_toolbar_button = Box::new(setter);
        self
    }

    /// Removes a connection-owned button when this socket renegotiates.
    pub fn with_toolbar_clearer(
        mut self,
        clearer: impl FnMut() -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.clear_toolbar_button = Box::new(clearer);
        self
    }

    pub fn with_popup(
        mut self,
        show: impl FnMut(u64, String, String, u64) -> Result<(), String> + Send + 'static,
        clear: impl FnMut() -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.show_popup = Box::new(show);
        self.clear_popup = Box::new(clear);
        self
    }

    pub fn with_popup_action(
        mut self,
        show: impl FnMut(u64, String, String, String, u64) -> Result<(), String> + Send + 'static,
    ) -> Self {
        self.show_popup_action = Box::new(show);
        self
    }
}

impl<'a> ExtensionConnectionAuthentication<'a> {
    /// Allows the documented bearer-claim development protocol.
    pub const fn unauthenticated() -> Self {
        Self {
            expected: None,
            authenticated_ready: None,
            runtime_start: None,
            runtime_events: None,
        }
    }

    /// Requires every hello to prove this core-generated credential.
    pub const fn required(expected: &'a str) -> Self {
        Self {
            expected: Some(expected),
            authenticated_ready: None,
            runtime_start: None,
            runtime_events: None,
        }
    }

    /// Signals core after the first valid handshake, before any operation can
    /// be handled on the connection.
    pub fn with_ready_notification(mut self, ready: mpsc::Sender<()>) -> Self {
        self.authenticated_ready = Some(ready);
        self
    }

    /// Installs core's one-shot session-start barrier for the authenticated
    /// child. The receiver is shared only because the listener accepts peers
    /// concurrently; only a peer that already proved `expected` can consume
    /// it through `RuntimeReady`.
    pub fn with_runtime_start_receiver(
        mut self,
        runtime_start: Arc<Mutex<mpsc::Receiver<()>>>,
    ) -> Self {
        self.runtime_start = Some(runtime_start);
        self
    }

    /// Installs the bounded, core-owned event stream for the authenticated
    /// child. Events are pulled one at a time only after `RuntimeStart`, so a
    /// fresh Wasm invocation has completed before another event can arrive.
    pub fn with_runtime_event_receiver(
        mut self,
        runtime_events: Arc<Mutex<mpsc::Receiver<ExtensionRuntimeEvent>>>,
    ) -> Self {
        self.runtime_events = Some(runtime_events);
        self
    }

    pub(super) fn expected(&self) -> Option<&str> {
        self.expected
    }

    pub(super) fn signal_ready(&self) {
        if let Some(ready) = &self.authenticated_ready {
            let _ = ready.send(());
        }
    }

    pub(super) fn wait_for_runtime_start(&self) -> Result<(), String> {
        let receiver = self.runtime_start.as_ref().ok_or_else(|| {
            "the core has no runtime-start barrier for this extension connection".to_string()
        })?;
        receiver
            .lock()
            .map_err(|_| "the core runtime-start barrier was poisoned".to_string())?
            .recv()
            .map_err(|_| "the core ended before its extension runtime could start".to_string())
    }

    /// Returns `Ok(None)` when core has deliberately ended its lifecycle
    /// stream, which is a normal host-shutdown condition rather than a
    /// recoverable extension operation failure.
    pub(super) fn wait_for_runtime_event(&self) -> Result<Option<ExtensionRuntimeEvent>, String> {
        let receiver = self.runtime_events.as_ref().ok_or_else(|| {
            "the core has no lifecycle event stream for this extension connection".to_string()
        })?;
        let receiver = receiver
            .lock()
            .map_err(|_| "the core lifecycle event stream was poisoned".to_string())?;
        Ok(receiver.recv().ok())
    }
}
