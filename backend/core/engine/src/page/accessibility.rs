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
use std::collections::HashSet;

const MAX_REGIONS: usize = 256;
const MAX_ANNOUNCEMENTS: usize = 64;
const MAX_TEXT_CHARS: usize = 4096;

#[derive(Clone)]
struct Region {
    id: NodeId,
    politeness: LivePoliteness,
    atomic: bool,
    relevant: Relevant,
    busy: bool,
    chunks: Vec<LiveText>,
    elements: HashSet<NodeId>,
    remaining: usize,
}

#[derive(Clone)]
struct LiveText {
    source: NodeId,
    container: NodeId,
    text: String,
}

impl Region {
    fn push(&mut self, source: NodeId, container: NodeId, text: &str, truncated: &mut bool) {
        let mut chars = text.chars();
        let text: String = chars.by_ref().take(self.remaining).collect();
        *truncated |= chars.next().is_some();
        self.remaining -= text.chars().count();
        if !text.is_empty() {
            self.chunks.push(LiveText {
                source,
                container,
                text,
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
    announcements: Vec<AccessibilityAnnouncement>,
    truncated: bool,
}
impl LiveRegions {
    fn refresh(&mut self, regions: Vec<Region>, truncated: bool) {
        self.announcements.clear();
        self.truncated = truncated;
        let mut next = HashMap::new();
        for mut region in regions {
            let old = self.observed.remove(&region.id);
            if region.busy {
                // Keep the last ready content, including updates accumulated
                // through several layouts while aria-busy remains true.
                if let Some(old) = old {
                    region.chunks = old.chunks;
                    region.elements = old.elements;
                } else {
                    region.chunks.clear();
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
                for chunk in &region.chunks {
                    let new_element = old
                        .as_ref()
                        .is_none_or(|old| !old.elements.contains(&chunk.container));
                    match previous.get(&chunk.source) {
                        None if region.relevant.text
                            || region.relevant.additions && new_element =>
                        {
                            changed.push(chunk.text.as_str())
                        }
                        Some(old) if *old != &chunk.text && region.relevant.text => {
                            changed.push(chunk.text.as_str())
                        }
                        _ => {}
                    }
                }
                if region.relevant.removals {
                    for chunk in old_chunks {
                        if !current.contains_key(&chunk.source) {
                            changed.push(chunk.text.as_str());
                        }
                    }
                }
                if !changed.is_empty() {
                    let text = if region.atomic {
                        region
                            .chunks
                            .iter()
                            .map(|chunk| chunk.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        changed.join(" ")
                    };
                    if !text.is_empty() {
                        if self.announcements.len() < MAX_ANNOUNCEMENTS {
                            self.revision =
                                self.revision.checked_add(1).expect("AT revision exhausted");
                            self.announcements.push(AccessibilityAnnouncement {
                                sequence: self.revision,
                                region_id: region.id.as_u64(),
                                text: bounded_text(&text, &mut self.truncated),
                                politeness: region.politeness,
                            });
                        } else {
                            self.truncated = true;
                        }
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
            relevant: Option<Relevant>,
            busy: bool,
        }
        let mut regions: Vec<Region> = Vec::new();
        let mut pending = vec![(self.doc.root(), Inherited::default(), 0)];
        let mut truncated = false;
        let mut visited = 0;
        while let Some((node, mut inherited, depth)) = pending.pop() {
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
                if let Some(value) = get("aria-atomic") {
                    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
                        inherited.atomic = Some(truth(Some(value)));
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
                    } else {
                        inherited.owner = Some(regions.len());
                        regions.push(Region {
                            id: node,
                            politeness,
                            atomic: inherited
                                .atomic
                                .unwrap_or(matches!(role.as_str(), "status" | "alert")),
                            relevant: inherited.relevant.unwrap_or_default(),
                            busy: inherited.busy,
                            chunks: Vec::new(),
                            elements: HashSet::new(),
                            remaining: MAX_TEXT_CHARS,
                        });
                    }
                }
                if let Some(index) = inherited.owner {
                    regions[index].busy |= inherited.busy;
                    regions[index].elements.insert(node);
                    if tag_name == "input" || tag_name == "select" {
                        if let Some(text) = self
                            .native_control_public_value(node)
                            .filter(|text| !text.is_empty())
                        {
                            regions[index].push(node, node, &text, &mut truncated);
                        }
                        continue;
                    }
                    if tag_name == "img" {
                        if let Some(text) = get("alt").filter(|text| !text.is_empty()) {
                            regions[index].push(node, node, text, &mut truncated);
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
                        &mut truncated,
                    );
                }
            }
            let children = self.doc.children(node).collect::<Vec<_>>();
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child, inherited, depth + 1)),
            );
        }
        (regions, truncated)
    }

    pub(super) fn refresh_accessibility(&mut self) {
        let (regions, truncated) = self.collect_live_regions();
        let blocked: HashSet<_> = self
            .live_regions
            .observed
            .values()
            .flat_map(|region| region.chunks.iter())
            .filter(|chunk| {
                [chunk.source, chunk.container]
                    .iter()
                    .any(|id| self.doc.contains(*id) && !self.accessibility_safe_content(*id))
            })
            .map(|chunk| chunk.source)
            .collect();
        for region in self.live_regions.observed.values_mut() {
            region
                .chunks
                .retain(|chunk| !blocked.contains(&chunk.source));
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
            announcements: self.live_regions.announcements.clone(),
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
            truncated: self.live_regions.truncated,
        }
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
