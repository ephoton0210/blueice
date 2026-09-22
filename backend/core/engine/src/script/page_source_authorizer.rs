// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned authority for external BlueTS page-script source graphs.
//!
//! HTML supplies only a raw `src` declaration. A page host must resolve it
//! under its own origin, policy, integrity, fetch/cache, and resource rules,
//! then return the resulting *closed* graph here. This module performs no I/O,
//! URL resolution, or fallback module lookup itself.

use super::{
    direct_page::DirectPageScriptKind, javascript::AuthorizedJavaScriptModuleGraph,
    CombinedPageScriptLanguage,
};
use crate::TabId;
use blueice_bluets::AuthorizedModuleLoader;
use std::fmt;

/// The core-only context a source authorizer receives for one external page
/// declaration. `document_url` and `declared_src` are untrusted page inputs;
/// an authorizer must validate and resolve them before returning a graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageScriptSourceRequest {
    pub tab_id: TabId,
    pub document_generation: u64,
    pub ordinal: u32,
    pub kind: DirectPageScriptKind,
    pub document_url: String,
    pub declared_src: String,
}

/// A complete, already-authorized source graph for one external declaration.
/// `entry` and every module identity are canonical host choices. The resolver
/// fingerprint is carried into BlueTS's compiler/cache identity so an artifact
/// cannot be reused under a different source-selection policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedPageScriptGraph {
    pub entry: String,
    pub loader: AuthorizedModuleLoader,
    pub resolver_fingerprint: String,
}

impl AuthorizedPageScriptGraph {
    /// Forms an authorized graph record. This validates only local structural
    /// invariants; URL/origin/integrity/fetch policy is the authorizer's job.
    pub fn new(
        entry: impl Into<String>,
        loader: AuthorizedModuleLoader,
        resolver_fingerprint: impl Into<String>,
    ) -> Result<Self, AuthorizedPageScriptGraphError> {
        let entry = entry.into();
        if entry.is_empty() || entry.contains('\0') {
            return Err(AuthorizedPageScriptGraphError::EmptyEntry);
        }
        let resolver_fingerprint = resolver_fingerprint.into();
        if resolver_fingerprint.trim().is_empty() {
            return Err(AuthorizedPageScriptGraphError::EmptyResolverFingerprint);
        }
        Ok(Self {
            entry,
            loader,
            resolver_fingerprint,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedPageScriptGraphError {
    EmptyEntry,
    EmptyResolverFingerprint,
}

impl fmt::Display for AuthorizedPageScriptGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEntry => formatter.write_str("authorized page script entry is empty"),
            Self::EmptyResolverFingerprint => {
                formatter.write_str("authorized page script resolver fingerprint is empty")
            }
        }
    }
}

impl std::error::Error for AuthorizedPageScriptGraphError {}

/// The only authority accepted for an external page declaration. Implementors
/// are core/page-host owned; a parsed document, BlueTS, and BlueJS never get a
/// reference to this trait and therefore cannot acquire source-loading power.
pub trait PageScriptSourceAuthorizer {
    /// Resolves and authorizes exactly one external declaration. Returning an
    /// error denies only that declaration; the page pipeline may continue with
    /// later declarations under its normal independent-error behavior.
    fn authorize(
        &mut self,
        request: &PageScriptSourceRequest,
    ) -> Result<AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError>;
}

/// A private authorizer failure. Its message is intentionally never copied to
/// a [`super::inline_runner::DirectPageScriptExecutionReport`], because it may
/// contain protected network or page-source detail useful only to the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageScriptSourceAuthorizationError {
    message: String,
}

impl PageScriptSourceAuthorizationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PageScriptSourceAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PageScriptSourceAuthorizationError {}

/// The exact core-owned context supplied when authorizing one external
/// declaration for the private out-of-process page child. `document_url` and
/// `declared_src` are page data, not pre-authorized identities: an
/// implementation must apply its own origin, integrity, fetch/cache, and
/// resource policy before it returns a graph.
///
/// The request's language includes its exact grammar kind, and the tab,
/// document generation, and DOM-order ordinal are all supplied by the live
/// core document. A page cannot construct this record, choose an authorizer,
/// or carry an authorization from one declaration or navigation to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfProcessPageScriptSourceRequest {
    pub tab_id: TabId,
    pub document_generation: u64,
    pub ordinal: u32,
    pub language: CombinedPageScriptLanguage,
    pub document_url: String,
    pub declared_src: String,
}

/// One complete graph selected by an out-of-process page-source authorizer.
///
/// The two variants deliberately reuse the existing JavaScript and BlueTS
/// closed-graph types. Core validates that the returned variant matches the
/// parser-selected declaration language before it copies exact canonical IDs,
/// source bytes, static edges, and resolver fingerprint into the private
/// child protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizedOutOfProcessPageScriptGraph {
    JavaScript(AuthorizedJavaScriptModuleGraph),
    BlueTs(AuthorizedPageScriptGraph),
}

/// The sole optional authority for external declarations in the supervised
/// child route. It is selected once when the executor is constructed, stored
/// privately, and queried immutably; page content cannot replace it or obtain
/// its fetch, cache, integrity, or resolver capabilities.
///
/// Returning an error denies only this declaration. Its private detail is
/// never copied to an execution report or sent to the child.
pub trait OutOfProcessPageScriptSourceAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>;
}

/// Private owner-side source-authorization failure for the child route.
///
/// The text may include policy, URL, cache, or integrity detail, so it must
/// not cross the source-free report or child-protocol boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfProcessPageScriptSourceAuthorizationError {
    message: String,
}

impl OutOfProcessPageScriptSourceAuthorizationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OutOfProcessPageScriptSourceAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OutOfProcessPageScriptSourceAuthorizationError {}
