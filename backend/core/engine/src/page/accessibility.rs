// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native AT reads the same DOM, privacy rules and layout as the page.
use super::*;
use blueice_css::Value;
use blueice_ipc::accessibility::{
    AccessibilityAnnouncement, AccessibilityContext, AccessibilitySnapshot, LivePoliteness,
};
use blueice_ipc::Bounds;
use std::collections::{HashSet, VecDeque};
use zeroize::Zeroize;

const MAX_REGIONS: usize = 256;
const MAX_ANNOUNCEMENTS: usize = 64;
const MAX_TEXT_CHARS: usize = 4096;
const MAX_ATOMIC_GROUPS: usize = 256;

#[derive(Clone)]
struct Region {
    id: NodeId,
    politeness: LivePoliteness,
    busy: bool,
    chunks: Vec<LiveText>,
    groups: HashMap<NodeId, AtomicGroup>,
    elements: HashSet<NodeId>,
    remaining: usize,
    truncated: bool,
}

#[derive(Clone)]
struct LiveText {
    source: NodeId,
    container: NodeId,
    text: String,
    order: usize,
    relevant: Relevant,
    atomic_owner: Option<NodeId>,
}

#[derive(Clone)]
struct AtomicGroup {
    start: usize,
    end: usize,
    order: usize,
    relevant: Relevant,
    label: crate::ai_snapshot::NativeAuthorLabel,
}
#[derive(Clone, Copy)]
struct LiveScope {
    relevant: Relevant,
    atomic_owner: Option<NodeId>,
    order: usize,
}

impl Region {
    fn push(
        &mut self,
        source: NodeId,
        container: NodeId,
        text: &str,
        scope: LiveScope,
        truncated: &mut bool,
    ) {
        let mut chars = text.chars();
        let text: String = chars.by_ref().take(self.remaining).collect();
        let clipped = chars.next().is_some();
        *truncated |= clipped;
        self.truncated |= clipped;
        self.remaining -= text.chars().count();
        if !text.is_empty() {
            self.chunks.push(LiveText {
                source,
                container,
                text,
                relevant: scope.relevant,
                atomic_owner: scope.atomic_owner,
                order: scope.order,
            });
        }
    }
}

#[derive(Clone, Copy)]
struct Relevant {
    additions: bool,
    text: bool,
    removals: bool,
}
impl Default for Relevant {
    fn default() -> Self {
        Self {
            additions: true,
            text: true,
            removals: false,
        }
    }
}
impl Relevant {
    fn parse(value: &str) -> Self {
        let tokens: Vec<_> = value.split_ascii_whitespace().collect();
        let all = tokens.iter().any(|token| token.eq_ignore_ascii_case("all"));
        Self {
            additions: all
                || tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("additions")),
            text: all
                || tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("text")),
            removals: all
                || tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case("removals")),
        }
    }
}

#[derive(Default)]
pub(super) struct LiveRegions {
    initialized: bool,
    observed: HashMap<NodeId, Region>,
    revision: u64,
    announcements: VecDeque<PendingAnnouncement>,
    acknowledged: u64,
    lost_through: u64,
    truncated: bool,
}
struct PendingAnnouncement {
    announcement: AccessibilityAnnouncement,
    sources: Vec<Contributor>,
    truncated: bool,
}
#[derive(Clone, Copy)]
struct Contributor {
    source: NodeId,
    container: NodeId,
    author_name: bool,
}
impl From<&LiveText> for Contributor {
    fn from(chunk: &LiveText) -> Self {
        Self {
            source: chunk.source,
            container: chunk.container,
            author_name: false,
        }
    }
}
impl Drop for PendingAnnouncement {
    fn drop(&mut self) {
        self.announcement.text.zeroize();
    }
}
impl LiveRegions {
    fn refresh(&mut self, regions: Vec<Region>, truncated: bool) {
        self.truncated = truncated;
        let mut next = HashMap::new();
        for mut region in regions {
            let old = self.observed.remove(&region.id);
            if region.busy {
                // Keep the last ready content, including updates accumulated
                // through several layouts while aria-busy remains true.
                if let Some(old) = old {
                    region.chunks = old.chunks;
                    region.groups = old.groups;
                    region.elements = old.elements;
                    region.truncated = old.truncated;
                } else {
                    region.chunks.clear();
                    region.groups.clear();
                    region.elements.clear();
                }
            } else if self.initialized {
                let old_chunks = old
                    .as_ref()
                    .map(|old| old.chunks.as_slice())
                    .unwrap_or_default();
                let previous: HashMap<_, _> = old_chunks
                    .iter()
                    .map(|chunk| (chunk.source, &chunk.text))
                    .collect();
                let current: HashMap<_, _> = region
                    .chunks
                    .iter()
                    .map(|chunk| (chunk.source, &chunk.text))
                    .collect();
                let mut changed = Vec::new();
                for (index, chunk) in region.chunks.iter().enumerate() {
                    let new_element = old
                        .as_ref()
                        .is_none_or(|old| !old.elements.contains(&chunk.container));
                    match previous.get(&chunk.source) {
                        None if chunk.relevant.text || chunk.relevant.additions && new_element => {
                            changed.push((index, chunk, false))
                        }
                        Some(old) if *old != &chunk.text && chunk.relevant.text => {
                            changed.push((index, chunk, false))
                        }
                        _ => {}
                    }
                }
                for (index, chunk) in old_chunks.iter().enumerate() {
                    if !chunk.text.is_empty()
                        && chunk.relevant.removals
                        && !current.contains_key(&chunk.source)
                    {
                        changed.push((index, chunk, true));
                    }
                }
                let mut groups = Vec::new();
                let mut emitted = HashSet::new();
                let mut plain = Vec::new();
                for (index, chunk, removed) in changed {
                    if let Some(owner) = chunk.atomic_owner {
                        let (group_region, group) = if let Some(group) = region.groups.get(&owner) {
                            (&region, group)
                        } else if let Some((old, group)) = old
                            .as_ref()
                            .and_then(|old| old.groups.get(&owner).map(|group| (old, group)))
                        {
                            (old, group)
                        } else {
                            continue;
                        };
                        if emitted.insert(owner) {
                            groups.push((owner, group_region, group));
                        }
                    } else {
                        plain.push((index, chunk, removed));
                    }
                }
                // Only author attributes inside the region cause a label update.
                // Mutating an external aria-labelledby target refreshes the next
                // expansion's name, but is not itself a live-region notification.
                let mut label_changes = region
                    .groups
                    .iter()
                    .filter(|(owner, group)| {
                        group.relevant.text
                            && old
                                .as_ref()
                                .and_then(|old| old.groups.get(owner))
                                .is_some_and(|old| {
                                    old.label.attributes != group.label.attributes
                                        && old.label.text != group.label.text
                                })
                    })
                    .collect::<Vec<_>>();
                label_changes.sort_by_key(|(_, group)| group.start);
                for (&owner, group) in label_changes {
                    if emitted.insert(owner) {
                        groups.push((owner, &region, group));
                    }
                }
                groups.sort_by_key(|(_, _, group)| {
                    (group.start, std::cmp::Reverse(group.end), group.order)
                });
                let mut segments = Vec::new();
                let mut covered = Vec::new();
                for (owner, group_region, group) in groups {
                    if covered.iter().any(|&(observed, start, end)| {
                        std::ptr::eq(observed, group_region)
                            && start <= group.start
                            && group.end <= end
                            && start < end
                    }) {
                        continue;
                    }
                    covered.push((group_region, group.start, group.end));
                    let mut parts = Vec::new();
                    let mut contributing = Vec::new();
                    if !group.label.text.is_empty() {
                        parts.push(group.label.text.as_str());
                        contributing.extend(group.label.sources.iter().map(|&source| {
                            Contributor {
                                source,
                                container: source,
                                author_name: true,
                            }
                        }));
                    }
                    contributing.push(Contributor {
                        source: owner,
                        container: owner,
                        author_name: false,
                    });
                    for chunk in &group_region.chunks[group.start..group.end] {
                        if !chunk.text.is_empty() {
                            parts.push(chunk.text.as_str());
                            contributing.push(Contributor::from(chunk));
                        }
                    }
                    let removed = !std::ptr::eq(group_region, &region);
                    segments.push((
                        (removed, group.order),
                        parts,
                        contributing,
                        group.label.truncated || group_region.truncated,
                    ));
                }
                for (index, chunk, removed) in plain {
                    let observed = if removed {
                        old.as_ref().unwrap()
                    } else {
                        &region
                    };
                    if covered.iter().any(|&(group_region, start, end)| {
                        std::ptr::eq(group_region, observed) && start <= index && index < end
                    }) {
                        continue;
                    }
                    segments.push((
                        (removed, chunk.order),
                        vec![chunk.text.as_str()],
                        vec![Contributor::from(chunk)],
                        observed.truncated,
                    ));
                }
                segments.sort_by_key(|segment| segment.0);
                if !segments.is_empty() {
                    let mut clipped = false;
                    let mut parts = Vec::new();
                    let mut contributing = Vec::new();
                    for (_, text, sources, truncated) in segments {
                        parts.extend(text);
                        contributing.extend(sources);
                        clipped |= truncated;
                    }
                    let text = parts.join(" ");
                    if !text.is_empty() {
                        self.revision =
                            self.revision.checked_add(1).expect("AT revision exhausted");
                        if self.announcements.len() == MAX_ANNOUNCEMENTS {
                            let removed = self.announcements.pop_front().unwrap();
                            self.lost_through = removed.announcement.sequence;
                        }
                        let text = bounded_text(&text, &mut clipped);
                        self.announcements.push_back(PendingAnnouncement {
                            announcement: AccessibilityAnnouncement {
                                sequence: self.revision,
                                region_id: region.id.as_u64(),
                                text,
                                politeness: region.politeness,
                            },
                            sources: contributing,
                            truncated: clipped,
                        });
                    }
                }
            }
            next.insert(region.id, region);
        }
        self.observed = next;
        self.initialized = true;
    }
}

fn bounded_text(text: &str, truncated: &mut bool) -> String {
    let mut chars = text.chars();
    let result = chars.by_ref().take(MAX_TEXT_CHARS).collect();
    *truncated |= chars.next().is_some();
    result
}

fn truth(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

impl Page {
    fn accessibility_contributor_safe(&self, contributor: &Contributor, region: NodeId) -> bool {
        [contributor.source, contributor.container]
            .iter()
            .all(|&id| {
                if contributor.author_name {
                    self.doc.contains(id) && self.accessibility_author_content(id, region)
                } else {
                    !self.doc.contains(id) || self.accessibility_announcement_content(id, region)
                }
            })
    }

    pub(crate) fn accessibility_author_content(&self, node: NodeId, region: NodeId) -> bool {
        if !self.accessibility_safe_content(node) {
            return false;
        }
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            if id == region {
                return self.accessibility_announcement_content(node, region);
            }
            ancestor = self.doc.parent(id);
        }
        // External references are names, not members of this live region.
        true
    }

    fn accessibility_announcement_content(&self, node: NodeId, region: NodeId) -> bool {
        if !self.accessibility_safe_content(node) {
            return false;
        }
        // Off content remains ordinary readable AX content. It may no longer
        // contribute to this live region, including its retained removals.
        // Stop at the owner: an explicit inner region overrides an off ancestor.
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            if id == region {
                break;
            }
            if element_attribute(&self.doc, id, "aria-live")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("off"))
            {
                return false;
            }
            ancestor = self.doc.parent(id);
        }
        true
    }

    pub(crate) fn accessibility_safe_content(&self, node: NodeId) -> bool {
        if !self.accessibility_exposed(node) {
            return false;
        }
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            if let NodeData::Element { tag_name, .. } = self.doc.data(id) {
                let private =
                    element_attribute(&self.doc, id, "autocomplete").is_some_and(|value| {
                        value.split_ascii_whitespace().any(|token| {
                            token.to_ascii_lowercase().starts_with("cc-")
                                || token.eq_ignore_ascii_case("one-time-code")
                        })
                    });
                let kind = element_attribute(&self.doc, id, "type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                if private
                    || tag_name == "input"
                        && matches!(
                            kind.as_str(),
                            "password" | "file" | "hidden" | "payment" | "creditcard"
                        )
                {
                    return false;
                }
            }
            ancestor = self.doc.parent(id);
        }
        true
    }

    pub(crate) fn accessibility_exposed(&self, node: NodeId) -> bool {
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            if let NodeData::Element {
                tag_name,
                attributes,
            } = self.doc.data(id)
            {
                if matches!(
                    tag_name.as_str(),
                    "head" | "script" | "style" | "template" | "noscript"
                ) || attributes
                    .iter()
                    .any(|(name, _)| matches!(name.as_str(), "hidden" | "inert"))
                    || truth(element_attribute(&self.doc, id, "aria-hidden"))
                {
                    return false;
                }
            }
            if self.styles.get(&id).is_some_and(|style| {
                style.display == "none" || style.opacity() == 0.0
                    || matches!(style.other.get("visibility"), Some(Value::Keyword(value)) if value == "hidden" || value == "collapse")
            }) { return false; }
            ancestor = self.doc.parent(id);
        }
        true
    }

    fn collect_live_regions(&self) -> (Vec<Region>, bool) {
        #[derive(Clone, Copy, Default)]
        struct Inherited {
            owner: Option<usize>,
            atomic: Option<bool>,
            atomic_owner: Option<NodeId>,
            relevant: Option<Relevant>,
            busy: bool,
        }
        enum Visit {
            Enter(NodeId, Inherited, usize),
            Exit(usize, NodeId),
        }
        let mut regions: Vec<Region> = Vec::new();
        let mut pending = vec![Visit::Enter(self.doc.root(), Inherited::default(), 0)];
        let labels = crate::ai_snapshot::native_label_index(self);
        let mut group_count = 0;
        let mut truncated = false;
        let mut visited = 0;
        while let Some(visit) = pending.pop() {
            let (node, mut inherited, depth) = match visit {
                Visit::Enter(node, inherited, depth) => (node, inherited, depth),
                Visit::Exit(index, owner) => {
                    let end = regions[index].chunks.len();
                    regions[index].groups.get_mut(&owner).unwrap().end = end;
                    continue;
                }
            };
            visited += 1;
            if visited > 50_000 {
                truncated = true;
                break;
            }
            if depth > 256 {
                truncated = true;
                continue;
            }
            if !self.accessibility_safe_content(node) {
                continue;
            }
            if let NodeData::Element { tag_name, .. } = self.doc.data(node) {
                let get = |name| element_attribute(&self.doc, node, name);
                let mut explicit_atomic = None;
                if let Some(value) = get("aria-atomic").map(str::trim) {
                    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
                        explicit_atomic = Some(truth(Some(value)));
                        inherited.atomic = explicit_atomic;
                    }
                }
                if let Some(value) = get("aria-relevant") {
                    inherited.relevant = Some(Relevant::parse(value));
                }
                inherited.busy |= truth(get("aria-busy"));
                let role = get("role")
                    .unwrap_or("")
                    .split_ascii_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let explicit = get("aria-live").map(|value| value.trim().to_ascii_lowercase());
                let polite = match explicit.as_deref() {
                    Some("off") => {
                        inherited.owner = None;
                        inherited.atomic_owner = None;
                        None
                    }
                    Some("polite") => Some(LivePoliteness::Polite),
                    Some("assertive") => Some(LivePoliteness::Assertive),
                    _ => match role.as_str() {
                        "alert" => Some(LivePoliteness::Assertive),
                        "status" | "log" => Some(LivePoliteness::Polite),
                        _ => None,
                    },
                };
                if let Some(politeness) = polite {
                    if regions.len() >= MAX_REGIONS {
                        truncated = true;
                        inherited.owner = None;
                        inherited.atomic_owner = None;
                    } else {
                        inherited.owner = Some(regions.len());
                        // A nested live owner bounds atomic expansion to its own
                        // content, even when an explicit setting is inherited.
                        explicit_atomic = Some(
                            inherited
                                .atomic
                                .unwrap_or(matches!(role.as_str(), "status" | "alert")),
                        );
                        inherited.atomic_owner = None;
                        regions.push(Region {
                            id: node,
                            politeness,
                            busy: inherited.busy,
                            chunks: Vec::new(),
                            groups: HashMap::new(),
                            elements: HashSet::new(),
                            remaining: MAX_TEXT_CHARS,
                            truncated: false,
                        });
                    }
                }
                if let Some(index) = inherited.owner {
                    match explicit_atomic {
                        Some(true) if group_count < MAX_ATOMIC_GROUPS => {
                            group_count += 1;
                            inherited.atomic_owner = Some(node);
                            let start = regions[index].chunks.len();
                            let label = crate::ai_snapshot::native_author_label(
                                self,
                                node,
                                &labels,
                                Some(regions[index].id),
                            );
                            regions[index].groups.insert(
                                node,
                                AtomicGroup {
                                    start,
                                    end: start,
                                    order: visited,
                                    relevant: inherited.relevant.unwrap_or_default(),
                                    label,
                                },
                            );
                            pending.push(Visit::Exit(index, node));
                        }
                        Some(true) => {
                            truncated = true;
                            // Do not turn an unsupported atomic group into a
                            // misleading partial, non-atomic announcement.
                            continue;
                        }
                        Some(false) => inherited.atomic_owner = None,
                        None => {}
                    }
                    regions[index].busy |= inherited.busy;
                    regions[index].elements.insert(node);
                    if tag_name == "input" || tag_name == "select" {
                        if let Some(text) = self
                            .native_control_public_value(node)
                            .filter(|text| !text.is_empty())
                        {
                            regions[index].push(
                                node,
                                node,
                                &text,
                                LiveScope {
                                    relevant: inherited.relevant.unwrap_or_default(),
                                    atomic_owner: inherited.atomic_owner,
                                    order: visited,
                                },
                                &mut truncated,
                            );
                        }
                        continue;
                    }
                    if tag_name == "img" {
                        if let Some(text) = get("alt").filter(|text| !text.is_empty()) {
                            regions[index].push(
                                node,
                                node,
                                text,
                                LiveScope {
                                    relevant: inherited.relevant.unwrap_or_default(),
                                    atomic_owner: inherited.atomic_owner,
                                    order: visited,
                                },
                                &mut truncated,
                            );
                        }
                    }
                }
            } else if let (NodeData::Text { data }, Some(index)) =
                (self.doc.data(node), inherited.owner)
            {
                let text = data.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    regions[index].push(
                        node,
                        self.doc.parent(node).unwrap_or(node),
                        &text,
                        LiveScope {
                            relevant: inherited.relevant.unwrap_or_default(),
                            atomic_owner: inherited.atomic_owner,
                            order: visited,
                        },
                        &mut truncated,
                    );
                }
            }
            let children = self.doc.children(node).collect::<Vec<_>>();
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| Visit::Enter(child, inherited, depth + 1)),
            );
        }
        // A traversal budget can stop before queued exit markers are visited.
        // Their collected public prefix is still a valid, explicitly clipped range.
        for visit in pending {
            if let Visit::Exit(index, owner) = visit {
                let end = regions[index].chunks.len();
                regions[index].groups.get_mut(&owner).unwrap().end = end;
            }
        }
        (regions, truncated)
    }

    pub(super) fn refresh_accessibility(&mut self) {
        let (regions, truncated) = self.collect_live_regions();
        let live: HashSet<_> = regions.iter().map(|region| region.id.as_u64()).collect();
        let safe_sequences: HashSet<_> = self
            .live_regions
            .announcements
            .iter()
            .filter(|pending| {
                live.contains(&pending.announcement.region_id)
                    && pending.sources.iter().all(|source| {
                        self.accessibility_contributor_safe(
                            source,
                            NodeId::from_u64(pending.announcement.region_id),
                        )
                    })
            })
            .map(|pending| pending.announcement.sequence)
            .collect();
        self.live_regions
            .announcements
            .retain(|pending| safe_sequences.contains(&pending.announcement.sequence));
        let blocked: HashSet<_> = self
            .live_regions
            .observed
            .iter()
            .flat_map(|(id, region)| region.chunks.iter().map(move |chunk| (*id, chunk)))
            .filter(|(region, chunk)| {
                [chunk.source, chunk.container].iter().any(|id| {
                    self.doc.contains(*id) && !self.accessibility_announcement_content(*id, *region)
                })
            })
            .map(|(region, chunk)| (region, chunk.source))
            .collect();
        let blocked_labels: HashSet<_> = self
            .live_regions
            .observed
            .iter()
            .flat_map(|(region, observed)| {
                observed
                    .groups
                    .iter()
                    .filter(|(_, group)| {
                        group.label.sources.iter().any(|&source| {
                            !self.accessibility_contributor_safe(
                                &Contributor {
                                    source,
                                    container: source,
                                    author_name: true,
                                },
                                *region,
                            )
                        })
                    })
                    .map(move |(owner, _)| (*region, *owner))
            })
            .collect();
        for (id, region) in &mut self.live_regions.observed {
            // Keep indices stable for the atomic ranges, and erase excluded text
            // before diffing removals or retaining a busy baseline.
            for chunk in &mut region.chunks {
                if blocked.contains(&(*id, chunk.source)) {
                    chunk.text.zeroize();
                    chunk.text.clear();
                }
            }
            for (owner, group) in &mut region.groups {
                if blocked_labels.contains(&(*id, *owner)) {
                    group.label.text.zeroize();
                    group.label.text.clear();
                    group.label.sources.clear();
                    group.label.truncated = false;
                }
            }
        }
        self.live_regions.refresh(regions, truncated);
    }

    pub(crate) fn accessibility_snapshot(
        &self,
        nodes: &[blueice_ipc::AiNode],
    ) -> AccessibilitySnapshot {
        AccessibilitySnapshot {
            document_generation: self.document_generation,
            revision: self.live_regions.revision,
            acknowledged_revision: self.live_regions.acknowledged,
            delivery_version: 1,
            announcements: self
                .live_regions
                .announcements
                .iter()
                .map(|pending| pending.announcement.clone())
                .collect(),
            hidden_nodes: nodes
                .iter()
                .filter(|node| !self.accessibility_exposed(NodeId::from_u64(node.id)))
                .map(|node| node.id)
                .collect(),
            names: nodes
                .iter()
                .filter_map(|node| {
                    let name = crate::ai_snapshot::native_name(self, NodeId::from_u64(node.id));
                    (name != node.name).then_some(blueice_ipc::accessibility::AccessibilityName {
                        node_id: node.id,
                        name,
                    })
                })
                .collect(),
            truncated: self.live_regions.truncated
                || self.live_regions.lost_through > self.live_regions.acknowledged
                || self
                    .live_regions
                    .announcements
                    .iter()
                    .any(|pending| pending.truncated),
        }
    }

    pub(crate) fn accessibility_acknowledge(
        &mut self,
        delivery: &blueice_ipc::accessibility::AccessibilityDelivery,
        source: u64,
    ) -> Result<blueice_ipc::accessibility::AccessibilityDelivery, String> {
        if delivery.version != 1
            || delivery.frame_source != source
            || delivery.document_generation != self.document_generation
            || delivery.document_generation == 0
            || delivery.revision > self.live_regions.revision
        {
            return Err("Stale or unsupported accessibility delivery context".into());
        }
        self.live_regions.acknowledged = self.live_regions.acknowledged.max(delivery.revision);
        let acknowledged = self.live_regions.acknowledged;
        self.live_regions
            .announcements
            .retain(|pending| pending.announcement.sequence > acknowledged);
        Ok(blueice_ipc::accessibility::AccessibilityDelivery {
            revision: acknowledged,
            ..*delivery
        })
    }

    pub(crate) fn accessibility_reveal(
        &mut self,
        context: &AccessibilityContext,
        source: u64,
    ) -> Result<Bounds, String> {
        if context.version != 1
            || context.frame_source != source
            || context.document_generation != self.document_generation
            || context.frame_generation != self.frame_generation
            || context.frame_generation == 0
        {
            return Err("Stale or unsupported accessibility navigation context".into());
        }
        let node = NodeId::from_u64(context.node_id);
        if !self.doc.contains(node) || !self.accessibility_exposed(node) {
            return Err("Accessibility target is unavailable".into());
        }
        let snapshot = self.snapshot(self.frame_generation, 0);
        let bounds = snapshot
            .nodes
            .iter()
            .find(|item| item.id == context.node_id)
            .map(|node| node.bounds)
            .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
            .ok_or("Accessibility target has no semantic layout")?;
        let bottom = bounds.y + bounds.height;
        let desired = if bounds.y < self.scroll_y {
            bounds.y
        } else if bottom > self.scroll_y + self.viewport_height {
            (bottom - self.viewport_height).min(bounds.y)
        } else {
            self.scroll_y
        };
        self.scroll_by(desired - self.scroll_y);
        Ok(bounds)
    }
}

#[cfg(test)]
#[path = "../../tests/support/accessibility_notifications.rs"]
mod tests;
