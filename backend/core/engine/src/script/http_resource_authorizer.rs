// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Startup-configured HTTP(S) authority for out-of-process page scripts.
//!
//! This is deliberately a *resource manifest*, not a browser loader.  Core
//! selects an immutable origin rule, a canonical URL-to-SHA-256 manifest, and
//! fixed resource limits before it constructs the page executor.  A document
//! may name a `src`, but it cannot cause a request unless that name resolves
//! to an entry already in the manifest.  Static imports are discovered by the
//! language parsers, resolved by this owner, and recursively admitted only
//! under the same rule.  The resulting JavaScript or BlueTS graph has no
//! fallback resolver.
//!
//! Fetches accept only a direct `200 OK`, identity-encoded, UTF-8 response
//! with an exact allowed MIME type and a bounded `Content-Length`. Redirects,
//! arbitrary headers, dynamic imports, import maps, credentials, query
//! strings, fragments, and encoded/ambiguous path spellings are all outside
//! this first production source authority.  In particular, this module never
//! hands HTTP, cache, URL, or manifest access to a page, the supervised child,
//! or MCP.

use super::{
    direct_page::DirectPageScriptKind,
    javascript::{
        AuthorizedJavaScriptModule, AuthorizedJavaScriptModuleGraph, AuthorizedJavaScriptResolution,
    },
    page_source_authorizer::{
        AuthorizedOutOfProcessPageScriptGraph, AuthorizedPageScriptGraph,
        OutOfProcessPageScriptSourceAuthorizationError, OutOfProcessPageScriptSourceAuthorizer,
        OutOfProcessPageScriptSourceRequest,
    },
    BlueJsPageScriptKind, CombinedPageScriptLanguage,
};
use blueice_bluejs::{parse_module as parse_javascript_module, ModuleType};
use blueice_bluets::{
    parse_module as parse_bluets_module, AuthorizedModule, AuthorizedModuleLoader,
    AuthorizedModuleResolution, Declaration,
};
use blueice_net::canonical_http_origin;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

/// Hard ceiling for owner-selected resource-manifest records.  The owner may
/// choose tighter limits but cannot accidentally make this first authority an
/// unbounded web fetcher.
const MAX_MANIFEST_RESOURCES: usize = 128;
const MAX_MODULES_PER_GRAPH: usize = 64;
const MAX_MODULE_DEPTH: usize = 16;
const MAX_MODULE_SOURCE_BYTES: usize = 256 * 1024;
const MAX_GRAPH_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_RESOURCE_URL_BYTES: usize = 2048;
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const AUTHORIZER_VERSION: &str = "core-page-http-resource-authorizer-v1";

/// A canonical same-origin rule selected by the core owner at startup.
///
/// [`Self::same_document_origin`] permits only the canonical origin of the
/// live document. [`Self::exact_origin`] permits one owner-chosen canonical
/// HTTP(S) origin (for example, a pinned application asset host). Neither
/// variant can be selected by frontend, page, child, or MCP input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpScriptResourceOriginRule(OriginRule);

#[derive(Debug, Clone, PartialEq, Eq)]
enum OriginRule {
    SameDocumentOrigin,
    ExactOrigin(String),
}

impl HttpScriptResourceOriginRule {
    /// Allows only canonical same-origin resources for each live document.
    pub fn same_document_origin() -> Self {
        Self(OriginRule::SameDocumentOrigin)
    }

    /// Allows exactly one canonical HTTP(S) origin selected by the core owner.
    ///
    /// The input itself must already be a canonical tuple origin such as
    /// `https://assets.example.test` or `http://127.0.0.1:8080`; accepting a
    /// path, query, fragment, or non-canonical spelling would obscure the
    /// startup policy identity.
    pub fn exact_origin(origin: impl Into<String>) -> Result<Self, HttpScriptResourcePolicyError> {
        let origin = origin.into();
        if canonical_http_origin(&origin).ok().as_deref() != Some(origin.as_str()) {
            return Err(HttpScriptResourcePolicyError::InvalidExactOrigin);
        }
        Ok(Self(OriginRule::ExactOrigin(origin)))
    }

    fn permits(&self, document_origin: &str, resource_origin: &str) -> bool {
        match &self.0 {
            OriginRule::SameDocumentOrigin => resource_origin == document_origin,
            OriginRule::ExactOrigin(origin) => resource_origin == origin,
        }
    }

    fn fingerprint_component(&self) -> String {
        match &self.0 {
            OriginRule::SameDocumentOrigin => "same-document-origin".to_string(),
            OriginRule::ExactOrigin(origin) => format!("exact-origin:{origin}"),
        }
    }
}

/// Immutable, owner-selected SHA-256 expectations for canonical resource
/// URLs. Every admitted entry and static dependency must appear here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpScriptIntegrityManifest {
    entries: BTreeMap<String, String>,
}

impl HttpScriptIntegrityManifest {
    /// Validates a fixed set of `(canonical_url, sha256:<lowercase-hex>)`
    /// records. This constructor is intended for core startup configuration,
    /// not page-provided script attributes.
    pub fn new(
        entries: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, HttpScriptResourcePolicyError> {
        let mut validated = BTreeMap::new();
        for (url, integrity) in entries {
            let canonical = canonical_http_resource_url(&url)
                .map_err(|_| HttpScriptResourcePolicyError::InvalidManifestUrl)?;
            if canonical != url {
                return Err(HttpScriptResourcePolicyError::InvalidManifestUrl);
            }
            if !is_sha256_integrity(&integrity) {
                return Err(HttpScriptResourcePolicyError::InvalidManifestIntegrity);
            }
            if validated.insert(url, integrity).is_some() {
                return Err(HttpScriptResourcePolicyError::DuplicateManifestUrl);
            }
            if validated.len() > MAX_MANIFEST_RESOURCES {
                return Err(HttpScriptResourcePolicyError::TooManyManifestResources);
            }
        }
        if validated.is_empty() {
            return Err(HttpScriptResourcePolicyError::EmptyManifest);
        }
        Ok(Self { entries: validated })
    }

    fn integrity_for(&self, canonical_url: &str) -> Option<&str> {
        self.entries.get(canonical_url).map(String::as_str)
    }

    fn fingerprint_component(&self) -> String {
        self.entries
            .iter()
            .map(|(url, integrity)| format!("{url}={integrity}"))
            .collect::<Vec<_>>()
            .join("|")
    }
}

/// Resource bounds selected before the HTTP source authority is installed.
/// Root depth is one, so `max_module_depth: 1` permits an entry only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpScriptResourceLimits {
    pub max_modules_per_graph: usize,
    pub max_module_depth: usize,
    pub max_module_source_bytes: usize,
    pub max_graph_source_bytes: usize,
}

impl Default for HttpScriptResourceLimits {
    fn default() -> Self {
        Self {
            max_modules_per_graph: 32,
            max_module_depth: 8,
            max_module_source_bytes: 128 * 1024,
            max_graph_source_bytes: 512 * 1024,
        }
    }
}

/// Immutable startup policy for [`HttpOutOfProcessPageScriptSourceAuthorizer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpScriptResourcePolicy {
    origin_rule: HttpScriptResourceOriginRule,
    integrity_manifest: HttpScriptIntegrityManifest,
    limits: HttpScriptResourceLimits,
    resolver_fingerprint: String,
}

impl HttpScriptResourcePolicy {
    /// Forms one fixed owner policy. The policy has no mutators: replacement
    /// requires constructing a new executor at a core-owned lifecycle point.
    pub fn new(
        origin_rule: HttpScriptResourceOriginRule,
        integrity_manifest: HttpScriptIntegrityManifest,
        limits: HttpScriptResourceLimits,
    ) -> Result<Self, HttpScriptResourcePolicyError> {
        validate_limits(&limits)?;
        let fingerprint_input = format!(
            "{AUTHORIZER_VERSION}|{}|{}|{}|{}|{}|{}",
            origin_rule.fingerprint_component(),
            integrity_manifest.fingerprint_component(),
            limits.max_modules_per_graph,
            limits.max_module_depth,
            limits.max_module_source_bytes,
            limits.max_graph_source_bytes,
        );
        Ok(Self {
            origin_rule,
            integrity_manifest,
            limits,
            resolver_fingerprint: format!(
                "{AUTHORIZER_VERSION}:{}",
                sha256_integrity(fingerprint_input.as_bytes())
            ),
        })
    }

    /// The deterministic resolver/cache-policy identity copied into every
    /// authorized graph. It changes when the origin rule, manifest, or any
    /// resource limit changes.
    pub fn resolver_fingerprint(&self) -> &str {
        &self.resolver_fingerprint
    }
}

/// Core startup-configured HTTP(S) source authority for the supervised child.
///
/// Its only mutable state is a private cache of already integrity-checked,
/// immutable source bytes. Cache keys include the canonical URL, expected
/// SHA-256, and language MIME lane, which makes cache reuse deterministic and
/// prevents an accepted JavaScript response from being reused as BlueTS (or
/// vice versa). The cache has no public read API.
pub struct HttpOutOfProcessPageScriptSourceAuthorizer {
    policy: HttpScriptResourcePolicy,
    cache: RefCell<BTreeMap<ResourceCacheKey, CachedResource>>,
}

impl HttpOutOfProcessPageScriptSourceAuthorizer {
    /// Installs one immutable core-owned HTTP(S) authorization policy.
    pub fn new(policy: HttpScriptResourcePolicy) -> Self {
        Self {
            policy,
            cache: RefCell::new(BTreeMap::new()),
        }
    }

    /// Returns the fixed resolver fingerprint for diagnostics-free owner
    /// setup tests. It grants neither the manifest nor cache contents.
    pub fn resolver_fingerprint(&self) -> &str {
        self.policy.resolver_fingerprint()
    }

    fn authorize_graph(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, ResourceAuthorizationFailure> {
        let document_origin = canonical_http_origin(&request.document_url)
            .map_err(|_| ResourceAuthorizationFailure::InvalidDocumentUrl)?;
        let entry = resolve_resource_url(&request.document_url, &request.declared_src, true)?;
        self.ensure_permitted_resource(&document_origin, &entry)?;

        let language = GraphLanguage::from_request(request.language);
        let mut builder = AuthorizedGraphBuilder::new(self, document_origin, language);
        builder.visit(entry.clone(), 1)?;
        let (modules, resolutions) = builder.finish();
        match language {
            GraphLanguage::JavaScript(kind) => {
                let modules = modules
                    .into_iter()
                    .map(|(id, source)| AuthorizedJavaScriptModule::new(id, source))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| ResourceAuthorizationFailure::InvalidGraph)?;
                let resolutions = resolutions
                    .into_iter()
                    .map(|(from, specifier, target)| {
                        AuthorizedJavaScriptResolution::new(from, specifier, target)
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| ResourceAuthorizationFailure::InvalidGraph)?;
                let graph = AuthorizedJavaScriptModuleGraph::new(
                    entry,
                    modules,
                    resolutions,
                    self.policy.resolver_fingerprint.clone(),
                )
                .map_err(|_| ResourceAuthorizationFailure::InvalidGraph)?;
                if matches!(kind, BlueJsPageScriptKind::Classic)
                    && graph.authorized_modules().count() != 1
                {
                    return Err(ResourceAuthorizationFailure::InvalidGraph);
                }
                Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(graph))
            }
            GraphLanguage::BlueTs(_) => {
                let modules = modules
                    .into_iter()
                    .map(|(id, source)| AuthorizedModule::new(id, source))
                    .collect::<Vec<_>>();
                let resolutions = resolutions
                    .into_iter()
                    .map(|(from, specifier, target)| {
                        AuthorizedModuleResolution::new(from, specifier, target)
                    })
                    .collect::<Vec<_>>();
                let loader = AuthorizedModuleLoader::new(modules, resolutions)
                    .map_err(|_| ResourceAuthorizationFailure::InvalidGraph)?;
                let graph = AuthorizedPageScriptGraph::new(
                    entry,
                    loader,
                    self.policy.resolver_fingerprint.clone(),
                )
                .map_err(|_| ResourceAuthorizationFailure::InvalidGraph)?;
                Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(graph))
            }
        }
    }

    fn ensure_permitted_resource(
        &self,
        document_origin: &str,
        resource_url: &str,
    ) -> Result<(), ResourceAuthorizationFailure> {
        let resource_origin = canonical_http_origin(resource_url)
            .map_err(|_| ResourceAuthorizationFailure::InvalidResourceUrl)?;
        if !self
            .policy
            .origin_rule
            .permits(document_origin, &resource_origin)
        {
            return Err(ResourceAuthorizationFailure::OriginDenied);
        }
        if self
            .policy
            .integrity_manifest
            .integrity_for(resource_url)
            .is_none()
        {
            return Err(ResourceAuthorizationFailure::MissingIntegrity);
        }
        Ok(())
    }

    fn fetch_resource(
        &self,
        document_origin: &str,
        resource_url: &str,
        language: GraphLanguage,
    ) -> Result<String, ResourceAuthorizationFailure> {
        self.ensure_permitted_resource(document_origin, resource_url)?;
        let integrity = self
            .policy
            .integrity_manifest
            .integrity_for(resource_url)
            .expect("permission check requires a manifest integrity record");
        let cache_key = ResourceCacheKey {
            canonical_url: resource_url.to_string(),
            expected_integrity: integrity.to_string(),
            mime_lane: language.mime_lane(),
        };
        if let Some(resource) = self.cache.borrow().get(&cache_key) {
            return Ok(resource.source.clone());
        }

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(FETCH_TIMEOUT))
            .proxy(None)
            .build()
            .into();
        let mut response = agent
            .get(resource_url)
            .call()
            .map_err(|_| ResourceAuthorizationFailure::FetchFailed)?;
        if response.status().as_u16() != 200 {
            return Err(ResourceAuthorizationFailure::HttpStatusRejected);
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(normalized_mime)
            .ok_or(ResourceAuthorizationFailure::MissingContentType)?;
        if !language.accepts_mime(content_type) {
            return Err(ResourceAuthorizationFailure::MimeRejected);
        }
        let content_encoding = response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok());
        if !matches!(content_encoding, None | Some("identity")) {
            return Err(ResourceAuthorizationFailure::ContentEncodingRejected);
        }
        let declared_length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(ResourceAuthorizationFailure::MissingContentLength)?;
        if declared_length > self.policy.limits.max_module_source_bytes {
            return Err(ResourceAuthorizationFailure::ModuleBytesExceeded);
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(
                self.policy
                    .limits
                    .max_module_source_bytes
                    .saturating_add(1)
                    .try_into()
                    .expect("fixed source-byte limits fit u64"),
            )
            .read_to_vec()
            .map_err(|_| ResourceAuthorizationFailure::FetchFailed)?;
        if bytes.len() != declared_length {
            return Err(ResourceAuthorizationFailure::ContentLengthMismatch);
        }
        if bytes.len() > self.policy.limits.max_module_source_bytes {
            return Err(ResourceAuthorizationFailure::ModuleBytesExceeded);
        }
        if sha256_integrity(&bytes) != integrity {
            return Err(ResourceAuthorizationFailure::IntegrityMismatch);
        }
        let source = String::from_utf8(bytes).map_err(|_| ResourceAuthorizationFailure::NonUtf8)?;
        self.cache.borrow_mut().insert(
            cache_key,
            CachedResource {
                source: source.clone(),
            },
        );
        Ok(source)
    }
}

impl OutOfProcessPageScriptSourceAuthorizer for HttpOutOfProcessPageScriptSourceAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        self.authorize_graph(request)
            .map_err(|error| OutOfProcessPageScriptSourceAuthorizationError::new(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResourceCacheKey {
    canonical_url: String,
    expected_integrity: String,
    mime_lane: &'static str,
}

#[derive(Debug, Clone)]
struct CachedResource {
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphLanguage {
    JavaScript(BlueJsPageScriptKind),
    BlueTs(DirectPageScriptKind),
}

type AuthorizedSourceRecords = Vec<(String, String)>;
type AuthorizedStaticResolutionRecords = Vec<(String, String, String)>;

impl GraphLanguage {
    fn from_request(language: CombinedPageScriptLanguage) -> Self {
        match language {
            CombinedPageScriptLanguage::JavaScript(kind) => Self::JavaScript(kind),
            CombinedPageScriptLanguage::BlueTs(kind) => Self::BlueTs(kind),
        }
    }

    fn mime_lane(self) -> &'static str {
        match self {
            Self::JavaScript(_) => "javascript",
            Self::BlueTs(_) => "bluets",
        }
    }

    fn accepts_mime(self, mime: &str) -> bool {
        match self {
            Self::JavaScript(_) => matches!(
                mime,
                "application/javascript"
                    | "text/javascript"
                    | "application/ecmascript"
                    | "text/ecmascript"
            ),
            Self::BlueTs(_) => matches!(
                mime,
                "application/typescript" | "text/typescript" | "application/x-typescript"
            ),
        }
    }

    fn permits_static_edges(self) -> bool {
        !matches!(
            self,
            Self::JavaScript(BlueJsPageScriptKind::Classic)
                | Self::BlueTs(DirectPageScriptKind::Classic)
        )
    }
}

struct AuthorizedGraphBuilder<'a> {
    authorizer: &'a HttpOutOfProcessPageScriptSourceAuthorizer,
    document_origin: String,
    language: GraphLanguage,
    modules: BTreeMap<String, String>,
    resolutions: BTreeMap<(String, String), String>,
    visiting: BTreeSet<String>,
    total_source_bytes: usize,
}

impl<'a> AuthorizedGraphBuilder<'a> {
    fn new(
        authorizer: &'a HttpOutOfProcessPageScriptSourceAuthorizer,
        document_origin: String,
        language: GraphLanguage,
    ) -> Self {
        Self {
            authorizer,
            document_origin,
            language,
            modules: BTreeMap::new(),
            resolutions: BTreeMap::new(),
            visiting: BTreeSet::new(),
            total_source_bytes: 0,
        }
    }

    fn visit(
        &mut self,
        canonical_url: String,
        depth: usize,
    ) -> Result<(), ResourceAuthorizationFailure> {
        if self.modules.contains_key(&canonical_url) || self.visiting.contains(&canonical_url) {
            return Ok(());
        }
        if depth > self.authorizer.policy.limits.max_module_depth {
            return Err(ResourceAuthorizationFailure::ModuleDepthExceeded);
        }
        if self.modules.len() >= self.authorizer.policy.limits.max_modules_per_graph {
            return Err(ResourceAuthorizationFailure::ModuleCountExceeded);
        }
        self.visiting.insert(canonical_url.clone());
        let source =
            self.authorizer
                .fetch_resource(&self.document_origin, &canonical_url, self.language)?;
        self.total_source_bytes = self
            .total_source_bytes
            .checked_add(source.len())
            .ok_or(ResourceAuthorizationFailure::GraphBytesExceeded)?;
        if self.total_source_bytes > self.authorizer.policy.limits.max_graph_source_bytes {
            return Err(ResourceAuthorizationFailure::GraphBytesExceeded);
        }
        let specifiers = static_specifiers(self.language, &canonical_url, &source)?;
        if !self.language.permits_static_edges() && !specifiers.is_empty() {
            return Err(ResourceAuthorizationFailure::StaticEdgesInClassicScript);
        }
        self.modules.insert(canonical_url.clone(), source);
        for specifier in specifiers {
            let target = resolve_resource_url(&canonical_url, &specifier, false)?;
            self.authorizer
                .ensure_permitted_resource(&self.document_origin, &target)?;
            self.visit(target.clone(), depth + 1)?;
            self.resolutions
                .insert((canonical_url.clone(), specifier), target);
        }
        self.visiting.remove(&canonical_url);
        Ok(())
    }

    fn finish(self) -> (AuthorizedSourceRecords, AuthorizedStaticResolutionRecords) {
        (
            self.modules.into_iter().collect(),
            self.resolutions
                .into_iter()
                .map(|((from, specifier), target)| (from, specifier, target))
                .collect(),
        )
    }
}

fn static_specifiers(
    language: GraphLanguage,
    canonical_url: &str,
    source: &str,
) -> Result<Vec<String>, ResourceAuthorizationFailure> {
    match language {
        GraphLanguage::JavaScript(BlueJsPageScriptKind::Classic) => Ok(Vec::new()),
        GraphLanguage::JavaScript(BlueJsPageScriptKind::Module) => {
            let module = parse_javascript_module(source)
                .map_err(|_| ResourceAuthorizationFailure::JavaScriptParseRejected)?;
            let mut specifiers = BTreeSet::new();
            for import in module.imports {
                if import.module_type != ModuleType::JavaScript {
                    return Err(ResourceAuthorizationFailure::UnsupportedModuleType);
                }
                specifiers.insert(import.module_request);
            }
            for export in module.exports {
                let (specifier, module_type) = match export {
                    blueice_bluejs::ExportEntry::Local { .. } => continue,
                    blueice_bluejs::ExportEntry::Indirect {
                        module_request,
                        module_type,
                        ..
                    }
                    | blueice_bluejs::ExportEntry::Star {
                        module_request,
                        module_type,
                    }
                    | blueice_bluejs::ExportEntry::Namespace {
                        module_request,
                        module_type,
                        ..
                    } => (module_request, module_type),
                };
                if module_type != ModuleType::JavaScript {
                    return Err(ResourceAuthorizationFailure::UnsupportedModuleType);
                }
                specifiers.insert(specifier);
            }
            Ok(specifiers.into_iter().collect())
        }
        GraphLanguage::BlueTs(kind) => {
            let module = parse_bluets_module(canonical_url, source)
                .map_err(|_| ResourceAuthorizationFailure::BlueTsParseRejected)?;
            let mut specifiers = BTreeSet::new();
            for declaration in module.declarations {
                match declaration {
                    Declaration::Import(import) => {
                        specifiers.insert(import.specifier);
                    }
                    Declaration::TypeExport(export) => {
                        if let Some(specifier) = export.specifier {
                            specifiers.insert(specifier);
                        }
                    }
                    _ => {}
                }
            }
            if matches!(kind, DirectPageScriptKind::Classic) && !specifiers.is_empty() {
                return Err(ResourceAuthorizationFailure::StaticEdgesInClassicScript);
            }
            Ok(specifiers.into_iter().collect())
        }
    }
}

fn validate_limits(limits: &HttpScriptResourceLimits) -> Result<(), HttpScriptResourcePolicyError> {
    if limits.max_modules_per_graph == 0
        || limits.max_module_depth == 0
        || limits.max_module_source_bytes == 0
        || limits.max_graph_source_bytes == 0
        || limits.max_modules_per_graph > MAX_MODULES_PER_GRAPH
        || limits.max_module_depth > MAX_MODULE_DEPTH
        || limits.max_module_source_bytes > MAX_MODULE_SOURCE_BYTES
        || limits.max_graph_source_bytes > MAX_GRAPH_SOURCE_BYTES
        || limits.max_graph_source_bytes < limits.max_module_source_bytes
    {
        return Err(HttpScriptResourcePolicyError::InvalidLimits);
    }
    Ok(())
}

fn normalized_mime(value: &str) -> &str {
    value.split(';').next().unwrap_or_default().trim()
}

fn resolve_resource_url(
    base_url: &str,
    reference: &str,
    allow_plain_relative: bool,
) -> Result<String, ResourceAuthorizationFailure> {
    if reference.is_empty()
        || reference.len() > MAX_RESOURCE_URL_BYTES
        || reference.contains(['\0', '\\', '?', '#'])
        || reference
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'%')
    {
        return Err(ResourceAuthorizationFailure::InvalidResourceReference);
    }
    let lower = reference.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return canonical_http_resource_url(reference)
            .map_err(|_| ResourceAuthorizationFailure::InvalidResourceReference);
    }
    if reference.starts_with("//") || reference.contains(':') {
        return Err(ResourceAuthorizationFailure::InvalidResourceReference);
    }
    let origin = canonical_http_origin(base_url)
        .map_err(|_| ResourceAuthorizationFailure::InvalidResourceReference)?;
    let base_uri = base_url
        .parse::<ureq::http::Uri>()
        .map_err(|_| ResourceAuthorizationFailure::InvalidResourceReference)?;
    let base_path = base_uri.path();
    let path = if reference.starts_with('/') {
        normalize_path(reference)?
    } else {
        if !allow_plain_relative
            && !matches!(reference, "." | "..")
            && !reference.starts_with("./")
            && !reference.starts_with("../")
        {
            return Err(ResourceAuthorizationFailure::BareStaticSpecifier);
        }
        let directory = base_path
            .rsplit_once('/')
            .map(|(prefix, _)| prefix)
            .unwrap_or("");
        normalize_path(&format!("{directory}/{reference}"))?
    };
    Ok(format!("{origin}{path}"))
}

fn canonical_http_resource_url(url: &str) -> Result<String, ResourceAuthorizationFailure> {
    if url.is_empty()
        || url.len() > MAX_RESOURCE_URL_BYTES
        || url.contains(['\0', '\\', '?', '#'])
        || url
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'%')
    {
        return Err(ResourceAuthorizationFailure::InvalidResourceUrl);
    }
    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|_| ResourceAuthorizationFailure::InvalidResourceUrl)?;
    if uri
        .authority()
        .is_none_or(|authority| authority.as_str().contains('@'))
    {
        return Err(ResourceAuthorizationFailure::InvalidResourceUrl);
    }
    let origin =
        canonical_http_origin(url).map_err(|_| ResourceAuthorizationFailure::InvalidResourceUrl)?;
    let path = normalize_path(uri.path())?;
    Ok(format!("{origin}{path}"))
}

fn normalize_path(path: &str) -> Result<String, ResourceAuthorizationFailure> {
    if !path.starts_with('/') || path.contains('\0') || path.contains('\\') || path.contains('%') {
        return Err(ResourceAuthorizationFailure::InvalidResourceReference);
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if segments.pop().is_none() {
                return Err(ResourceAuthorizationFailure::PathEscapesRoot);
            }
            continue;
        }
        if !segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~'))
        {
            return Err(ResourceAuthorizationFailure::InvalidResourceReference);
        }
        segments.push(segment);
    }
    Ok(format!("/{}", segments.join("/")))
}

fn is_sha256_integrity(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Returns a manifest-compatible SHA-256 integrity value for fixed owner
/// configuration or tests. It is not exposed through page-host IPC.
pub fn sha256_integrity(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

// A small fixed SHA-256 implementation avoids making integrity a best-effort
// non-cryptographic fingerprint or adding another ambient hashing service to
// the page host. It follows FIPS 180-4's single-block compression primitive.
fn sha256(input: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while !(padded.len() + 8).is_multiple_of(64) {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    let mut hash = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes(chunk[offset..offset + 4].try_into().expect("chunk width"));
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for index in 0..64 {
            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
        hash[5] = hash[5].wrapping_add(f);
        hash[6] = hash[6].wrapping_add(g);
        hash[7] = hash[7].wrapping_add(h);
    }
    let mut output = [0; 32];
    for (index, word) in hash.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpScriptResourcePolicyError {
    InvalidExactOrigin,
    InvalidManifestUrl,
    InvalidManifestIntegrity,
    DuplicateManifestUrl,
    TooManyManifestResources,
    EmptyManifest,
    InvalidLimits,
}

impl fmt::Display for HttpScriptResourcePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidExactOrigin => {
                formatter.write_str("resource policy exact origin is invalid")
            }
            Self::InvalidManifestUrl => formatter.write_str("resource manifest URL is invalid"),
            Self::InvalidManifestIntegrity => {
                formatter.write_str("resource manifest integrity must be lowercase sha256 hex")
            }
            Self::DuplicateManifestUrl => {
                formatter.write_str("resource manifest URL is duplicated")
            }
            Self::TooManyManifestResources => {
                formatter.write_str("resource manifest exceeds the fixed resource limit")
            }
            Self::EmptyManifest => formatter.write_str("resource manifest must not be empty"),
            Self::InvalidLimits => formatter.write_str("resource policy limits are invalid"),
        }
    }
}

impl std::error::Error for HttpScriptResourcePolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceAuthorizationFailure {
    InvalidDocumentUrl,
    InvalidResourceReference,
    InvalidResourceUrl,
    PathEscapesRoot,
    BareStaticSpecifier,
    OriginDenied,
    MissingIntegrity,
    FetchFailed,
    HttpStatusRejected,
    MissingContentType,
    MimeRejected,
    ContentEncodingRejected,
    MissingContentLength,
    ContentLengthMismatch,
    ModuleBytesExceeded,
    GraphBytesExceeded,
    IntegrityMismatch,
    NonUtf8,
    ModuleDepthExceeded,
    ModuleCountExceeded,
    JavaScriptParseRejected,
    BlueTsParseRejected,
    UnsupportedModuleType,
    StaticEdgesInClassicScript,
    InvalidGraph,
}

impl fmt::Display for ResourceAuthorizationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // These strings stay owner-private: the caller maps every failure to
        // the existing source-free fixed page-report category.
        formatter.write_str(match self {
            Self::InvalidDocumentUrl => "document URL is not an HTTP(S) origin",
            Self::InvalidResourceReference => "resource reference is not in the static URL subset",
            Self::InvalidResourceUrl => "resource URL is invalid",
            Self::PathEscapesRoot => "resource path escapes the origin root",
            Self::BareStaticSpecifier => "bare static module specifier is denied",
            Self::OriginDenied => "resource origin is not permitted by the startup policy",
            Self::MissingIntegrity => "resource is absent from the owner integrity manifest",
            Self::FetchFailed => "resource request failed",
            Self::HttpStatusRejected => "resource response status is not 200",
            Self::MissingContentType => "resource response has no allowed content type",
            Self::MimeRejected => "resource response MIME type is not permitted",
            Self::ContentEncodingRejected => "resource response content encoding is not identity",
            Self::MissingContentLength => "resource response has no bounded content length",
            Self::ContentLengthMismatch => "resource response length does not match content length",
            Self::ModuleBytesExceeded => "resource source bytes exceed the module limit",
            Self::GraphBytesExceeded => "resource graph bytes exceed the graph limit",
            Self::IntegrityMismatch => {
                "resource source does not match the owner integrity manifest"
            }
            Self::NonUtf8 => "resource source is not UTF-8",
            Self::ModuleDepthExceeded => "resource graph exceeds the module depth limit",
            Self::ModuleCountExceeded => "resource graph exceeds the module count limit",
            Self::JavaScriptParseRejected => "JavaScript module syntax could not be inspected",
            Self::BlueTsParseRejected => "BlueTS module syntax could not be inspected",
            Self::UnsupportedModuleType => "non-JavaScript module attributes are denied",
            Self::StaticEdgesInClassicScript => "classic script declares static module edges",
            Self::InvalidGraph => "authorized resource graph is structurally invalid",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_integrity_matches_fips_vectors() {
        assert_eq!(
            sha256_integrity(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_integrity(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn resource_urls_are_canonical_and_static_path_only() {
        assert_eq!(
            canonical_http_resource_url("HTTP://EXAMPLE.test:80/assets/./main.js").unwrap(),
            "http://example.test/assets/main.js"
        );
        assert_eq!(
            resolve_resource_url(
                "https://example.test/app/index.html?ignored=1",
                "./scripts/../main.js",
                true,
            )
            .unwrap(),
            "https://example.test/app/main.js"
        );
        assert!(matches!(
            resolve_resource_url("https://example.test/app/index.html", "pkg", false),
            Err(ResourceAuthorizationFailure::BareStaticSpecifier)
        ));
        assert!(canonical_http_resource_url("https://example.test/a%2fb.js").is_err());
    }

    #[test]
    fn manifest_and_policy_require_canonical_owner_configuration() {
        assert!(HttpScriptResourceOriginRule::exact_origin("https://EXAMPLE.test").is_err());
        let exact =
            HttpScriptResourceOriginRule::exact_origin("https://assets.example.test").unwrap();
        assert!(exact.permits("https://page.example.test", "https://assets.example.test"));
        assert!(!exact.permits("https://page.example.test", "https://other.example.test"));
        assert!(HttpScriptIntegrityManifest::new([(
            "https://example.test/a.js".to_string(),
            "sha256:not-a-digest".to_string(),
        )])
        .is_err());
        let manifest = HttpScriptIntegrityManifest::new([(
            "https://example.test/a.js".to_string(),
            sha256_integrity(b"a"),
        )])
        .unwrap();
        let policy = HttpScriptResourcePolicy::new(
            HttpScriptResourceOriginRule::same_document_origin(),
            manifest.clone(),
            HttpScriptResourceLimits::default(),
        )
        .unwrap();
        assert!(policy
            .resolver_fingerprint()
            .starts_with("core-page-http-resource-authorizer-v1:sha256:"));
        let zero_depth = HttpScriptResourceLimits {
            max_module_depth: 0,
            ..HttpScriptResourceLimits::default()
        };
        assert!(HttpScriptResourcePolicy::new(
            HttpScriptResourceOriginRule::same_document_origin(),
            manifest,
            zero_depth,
        )
        .is_err());
    }
}
