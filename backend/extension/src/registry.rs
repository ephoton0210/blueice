// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// An inclusive API-version interval a host supports for one capability.
/// A capability grant and a supported version are deliberately separate:
/// an extension may use a known API version without being authorized to
/// invoke that capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityVersionWindow {
    min_inclusive: u32,
    max_inclusive: u32,
}

impl CapabilityVersionWindow {
    /// Creates a valid inclusive version window, or returns `None` when
    /// its bounds are inverted.
    pub const fn new(min_inclusive: u32, max_inclusive: u32) -> Option<Self> {
        if min_inclusive <= max_inclusive {
            Some(Self {
                min_inclusive,
                max_inclusive,
            })
        } else {
            None
        }
    }

    /// Whether `version` is supported by this interval.
    pub const fn contains(self, version: u32) -> bool {
        self.min_inclusive <= version && version <= self.max_inclusive
    }

    /// The lower inclusive API-version bound.
    pub const fn min_inclusive(self) -> u32 {
        self.min_inclusive
    }

    /// The upper inclusive API-version bound.
    pub const fn max_inclusive(self) -> u32 {
        self.max_inclusive
    }
}

/// A native one-shot decision must not remain usable indefinitely if the
/// authenticated host stalls before running its queued invocation.
pub(super) const RUNTIME_EPHEMERAL_TTL: Duration = Duration::from_secs(10);

/// Which capabilities each connected extension has been granted. The registry
/// is keyed by an installed package's derived ID when used by
/// `blueice-core`/`registry_for_installed_extension`; the hardcoded
/// [`ExtensionRegistry::minimal_slice`] remains only a protocol-test fallback.
/// A derived ID establishes exact package membership and grants, but it is not
/// connection credentials. Core's optional host-spawned path adds that separate
/// boundary; standalone/manual protocol development intentionally does not.
pub struct ExtensionRegistry {
    grants: HashMap<String, HashSet<String>>,
    optional_declarations: HashMap<String, HashMap<String, OptionalGrantState>>,
    ephemeral_declarations: HashMap<String, HashMap<String, Mutex<EphemeralSlot>>>,
    supported_versions: HashMap<String, CapabilityVersionWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EphemeralLease {
    pub(super) ticket: String,
    pub(super) tab_id: u64,
    pub(super) document_epoch: u64,
    pub(super) expires_at: Instant,
}

#[derive(Default)]
pub(super) struct EphemeralSlot {
    pub(super) lease: Option<EphemeralLease>,
}

#[derive(Default)]
pub(super) struct OptionalGrantState {
    /// Serializes transitions with a reviewed effect's synchronous core
    /// acknowledgement. Readers in the core session use `published` instead
    /// of taking this lock, avoiding a writer-preference deadlock.
    pub(super) serialize: Mutex<()>,
    /// Low bit is the grant; upper bits are its monotonic generation.
    pub(super) published: AtomicU64,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionRegistry {
    /// An empty registry: no extension_id is granted anything.
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            optional_declarations: HashMap::new(),
            ephemeral_declarations: HashMap::new(),
            supported_versions: HashMap::new(),
        }
    }

    /// Registers the complete set of capability API windows this Phase 9 host
    /// understands. Installation grants remain a separate operation, so an
    /// extension cannot turn host support into authority merely by declaring a
    /// capability in its manifest or handshake.
    pub fn with_supported_capabilities() -> Self {
        let mut registry = Self::new();
        let v1_to_v2 = CapabilityVersionWindow::new(1, 2).expect("literal version window is valid");
        let v1_to_v3 = CapabilityVersionWindow::new(1, 3).expect("literal version window is valid");
        let v1_to_v6 = CapabilityVersionWindow::new(1, 6).expect("literal version window is valid");
        let v1_to_v9 = CapabilityVersionWindow::new(1, 9).expect("literal version window is valid");
        registry.register_capability_version_window(CAPABILITY_DOM_READ, v1_to_v3);
        registry.register_capability_version_window(CAPABILITY_DOM_WRITE, v1_to_v9);
        registry.register_capability_version_window(CAPABILITY_NETWORK_INTERCEPT, v1_to_v6);
        registry.register_capability_version_window(CAPABILITY_NETWORK_OBSERVE, v1_to_v2);
        registry.register_capability_version_window(CAPABILITY_UI_INJECT, v1_to_v3);
        registry.register_capability_version_window(CAPABILITY_STORAGE, v1_to_v3);
        registry
    }

    /// Registers the API-version window this host implements for a
    /// capability. Registering a capability does not grant it to any
    /// extension; use [`Self::grant`] for that separate decision.
    pub fn register_capability_version_window(
        &mut self,
        capability: impl Into<String>,
        window: CapabilityVersionWindow,
    ) {
        self.supported_versions.insert(capability.into(), window);
    }

    /// Grants `capability` to `extension_id`, in addition to whatever
    /// it already holds.
    pub fn grant(&mut self, extension_id: impl Into<String>, capability: impl Into<String>) {
        self.grants
            .entry(extension_id.into())
            .or_default()
            .insert(capability.into());
    }

    /// Retains an install-validated optional declaration without granting it.
    /// Only the installed-manifest loader should seed this table. No guest or
    /// ordinary client message can write to it.
    pub(crate) fn declare_optional(
        &mut self,
        extension_id: impl Into<String>,
        capability: impl Into<String>,
    ) {
        self.optional_declarations
            .entry(extension_id.into())
            .or_default()
            .entry(capability.into())
            .or_default();
    }

    /// Retains a manifest-validated runtime-ephemeral declaration without
    /// adding it to ordinary capability grants. Only the installed loader
    /// seeds this table; negotiation alone still carries no authority.
    pub(crate) fn declare_runtime_ephemeral(
        &mut self,
        extension_id: impl Into<String>,
        capability: impl Into<String>,
    ) {
        self.ephemeral_declarations
            .entry(extension_id.into())
            .or_default()
            .entry(capability.into())
            .or_default();
    }

    /// Core-owned primitive for a future trusted native gesture. The caller
    /// must independently verify the live tab/document identity immediately
    /// before arming. This does not affect `has_capability` or grant a
    /// connection-wide permission: exactly one matching operation may consume
    /// the lease through `consume_runtime_ephemeral`.
    pub fn arm_runtime_ephemeral(
        &self,
        extension_id: &str,
        capability: &str,
        tab_id: u64,
        document_epoch: u64,
    ) -> Result<String, String> {
        if tab_id == 0 {
            return Err("an ephemeral lease requires a live tab ID".to_string());
        }
        let state = self.ephemeral_state(extension_id, capability)?;
        let mut slot = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ticket = new_ephemeral_ticket()?;
        slot.lease = Some(EphemeralLease {
            ticket: ticket.clone(),
            tab_id,
            document_epoch,
            expires_at: Instant::now() + RUNTIME_EPHEMERAL_TTL,
        });
        Ok(ticket)
    }

    /// Inspection reveals only whether a lease remains, never its bearer.
    /// Only `ArmEphemeral`'s private parent reply carries the secret token.
    pub fn has_unspent_runtime_ephemeral_lease(
        &self,
        extension_id: &str,
        capability: &str,
    ) -> bool {
        self.ephemeral_state(extension_id, capability)
            .ok()
            .is_some_and(|state| {
                let mut slot = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if slot
                    .lease
                    .as_ref()
                    .is_some_and(|lease| Instant::now() >= lease.expires_at)
                {
                    slot.lease = None;
                }
                slot.lease.is_some()
            })
    }

    pub fn has_runtime_ephemeral_declaration(&self, extension_id: &str, capability: &str) -> bool {
        self.ephemeral_state(extension_id, capability).is_ok()
    }

    /// Attempts one exact tab/document use. A request for a different tab
    /// cannot spend the lease; a changed epoch on the same tab discards it so
    /// a navigation or history restore cannot revive the old authority.
    pub fn consume_runtime_ephemeral(
        &self,
        extension_id: &str,
        capability: &str,
        expected_ticket: &str,
        tab_id: u64,
        document_epoch: u64,
    ) -> bool {
        let Ok(state) = self.ephemeral_state(extension_id, capability) else {
            return false;
        };
        let mut slot = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot
            .lease
            .as_ref()
            .is_some_and(|lease| Instant::now() >= lease.expires_at)
        {
            slot.lease = None;
            return false;
        }
        match slot.lease.as_ref() {
            Some(current)
                if constant_time_authentication_matches(&current.ticket, expected_ticket)
                    && current.tab_id == tab_id =>
            {
                let correct_epoch = current.document_epoch == document_epoch;
                slot.lease = None;
                correct_epoch
            }
            _ => false,
        }
    }

    /// Withdraws an unspent lease, including when the private parent pipe
    /// disappears or the trusted native window is lost.
    pub fn revoke_runtime_ephemeral(
        &self,
        extension_id: &str,
        capability: &str,
    ) -> Result<bool, String> {
        let state = self.ephemeral_state(extension_id, capability)?;
        Ok(state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lease
            .take()
            .is_some())
    }

    pub(super) fn ephemeral_state(
        &self,
        extension_id: &str,
        capability: &str,
    ) -> Result<&Mutex<EphemeralSlot>, String> {
        self.ephemeral_declarations
            .get(extension_id)
            .and_then(|caps| caps.get(capability))
            .ok_or_else(|| {
                format!(
                "{capability} is not an installed runtime-ephemeral declaration for {extension_id}"
            )
            })
    }

    /// In-process transition reserved for a separately authenticated human
    /// approval channel. No guest or public IPC message can invoke it.
    /// Returns whether state changed.
    pub fn grant_optional(&self, extension_id: &str, capability: &str) -> Result<bool, String> {
        let state = self.optional_state(extension_id, capability)?;
        let _serial = state
            .serialize
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let encoded = state.published.load(Ordering::Acquire);
        if encoded == u64::MAX - 1 {
            return Err("optional grant generation is exhausted".to_string());
        }
        if encoded & 1 == 1 {
            return Ok(false);
        }
        state.published.store(encoded + 1, Ordering::Release);
        Ok(true)
    }

    /// Every later operation on an already-negotiated connection rechecks
    /// this state. A changed grant also advances its generation, so a future
    /// regrant cannot reactivate persistent effects from the old generation.
    pub fn revoke_optional(&self, extension_id: &str, capability: &str) -> Result<bool, String> {
        let state = self.optional_state(extension_id, capability)?;
        let _serial = state
            .serialize
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let encoded = state.published.load(Ordering::Acquire);
        if encoded & 1 == 0 {
            return Ok(false);
        }
        state.published.store(
            encoded.checked_add(1).unwrap_or(u64::MAX - 1),
            Ordering::Release,
        );
        Ok(true)
    }

    pub(super) fn optional_state(
        &self,
        extension_id: &str,
        capability: &str,
    ) -> Result<&OptionalGrantState, String> {
        self.optional_declarations
            .get(extension_id)
            .and_then(|caps| caps.get(capability))
            .ok_or_else(|| {
                format!("{capability} is not an installed optional declaration for {extension_id}")
            })
    }

    /// A live capability's generation is a lease for core-owned effects.
    /// `None` is fail-closed for an ungranted tier. This is a lock-free read so
    /// a core session can check it while a registration guard is held by the
    /// extension worker waiting for that session's acknowledgement.
    /// A later regrant always has a different generation from an old lease.
    pub fn capability_generation(&self, extension_id: &str, capability: &str) -> Option<u64> {
        if self
            .grants
            .get(extension_id)
            .is_some_and(|caps| caps.contains(capability))
        {
            return Some(0);
        }
        let encoded = self
            .optional_declarations
            .get(extension_id)?
            .get(capability)?
            .published
            .load(Ordering::Acquire);
        (encoded & 1 == 1).then_some(encoded >> 1)
    }

    /// Runs one already-reviewed, core-owned effect only while its original
    /// grant generation is still current. The serialization guard spans the effect's
    /// synchronous core acknowledgement, so a concurrent revoke cannot
    /// complete in between the final permission check and publication.
    /// Callers must not grant or revoke this registry from `effect`.
    pub fn with_stable_capability<T>(
        &self,
        extension_id: &str,
        capability: &str,
        expected_generation: u64,
        effect: impl FnOnce() -> T,
    ) -> Result<T, String> {
        if self
            .grants
            .get(extension_id)
            .is_some_and(|caps| caps.contains(capability))
        {
            return (expected_generation == 0)
                .then(effect)
                .ok_or_else(|| grant_changed_reason(capability));
        }
        let state = self.optional_state(extension_id, capability)?;
        let _serial = state
            .serialize
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let encoded = state.published.load(Ordering::Acquire);
        if encoded & 1 == 0 || encoded >> 1 != expected_generation {
            return Err(grant_changed_reason(capability));
        }
        Ok(effect())
    }

    /// The actual enforcement point: does `extension_id` currently hold
    /// `capability`? An unrecognized `extension_id` (never granted
    /// anything) simply has no capabilities -- see this crate's module
    /// docs for why an unknown identity isn't rejected outright at
    /// handshake time.
    pub fn has_capability(&self, extension_id: &str, capability: &str) -> bool {
        self.capability_generation(extension_id, capability)
            .is_some()
    }

    /// Returns why a declared API version is unavailable, if it cannot
    /// be negotiated with this host.
    pub fn unsupported_capability_version(
        &self,
        capability: &str,
        version: u32,
    ) -> Option<UnsupportedCapabilityVersion> {
        match self.supported_versions.get(capability).copied() {
            None => Some(UnsupportedCapabilityVersion::UnknownCapability),
            Some(window) if !window.contains(version) => {
                Some(UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: window.min_inclusive(),
                    max_inclusive: window.max_inclusive(),
                })
            }
            Some(_) => None,
        }
    }

    /// Validates all declarations in one `Hello`, retaining every
    /// independently unsupported entry for the structured handshake
    /// reply. This never rejects a compatible declaration merely because
    /// another capability on the same connection is unavailable.
    pub fn unsupported_capability_versions(
        &self,
        declared_versions: &BTreeMap<String, u32>,
    ) -> BTreeMap<String, UnsupportedCapabilityVersion> {
        declared_versions
            .iter()
            .filter_map(|(capability, version)| {
                self.unsupported_capability_version(capability, *version)
                    .map(|problem| (capability.clone(), problem))
            })
            .collect()
    }

    /// Seeds the hardcoded single-extension grant this minimal slice
    /// ships: [`MINIMAL_SLICE_EXTENSION_ID`] gets [`CAPABILITY_DOM_READ`]
    /// only -- deliberately *not* [`CAPABILITY_DOM_WRITE`] or
    /// [`CAPABILITY_NETWORK_INTERCEPT`], so a `DomWrite` or
    /// `NetworkIntercept` attempt is a concrete proof of server-side
    /// denial.
    pub fn minimal_slice() -> Self {
        let mut registry = Self::with_supported_capabilities();
        registry.grant(MINIMAL_SLICE_EXTENSION_ID, CAPABILITY_DOM_READ);
        registry
    }
}

pub(super) fn grant_changed_reason(capability: &str) -> String {
    format!("{capability} grant changed during review")
}

/// A lease bearer must be unpredictable to the extension before the trusted
/// gesture delivers it. Use the operating system's cryptographic entropy
/// source; entropy failure rejects arming rather than issuing
/// a predictable fallback token.
pub(super) fn new_ephemeral_ticket() -> Result<String, String> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random)
        .map_err(|error| format!("could not obtain ephemeral lease entropy: {error}"))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut ticket = String::with_capacity(random.len() * 2);
    for byte in random {
        ticket.push(HEX[(byte >> 4) as usize] as char);
        ticket.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(ticket)
}
