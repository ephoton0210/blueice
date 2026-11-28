// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Restyling a page after a change by redoing only what the document says changed
//! (`blueice_css::cascade_incremental`), falling back to a full cascade whenever the
//! inputs to the rules themselves may have changed.

use super::Page;
use blueice_css::{cascade_incremental_report_with_containers, MediaEnvironment, Origin};
use blueice_dom::{Changes, Document, NodeData, NodeId};

/// What the last full cascade was computed from, besides the document itself.
#[derive(Clone, Copy)]
pub(super) struct StyleEpoch {
    media: MediaEnvironment,
    /// `<style>` and `<link>` elements in the document then.
    sheet_elements: usize,
}

/// A `<style>`, a `<base>` (it moves where linked sheets come from), or a `<link>` that
/// is a stylesheet: scripts insert `preload` and `icon` links all the time and those
/// feed no rule.
fn is_sheet_element(doc: &Document, node: NodeId) -> bool {
    match doc.data(node) {
        NodeData::Element {
            tag_name,
            attributes,
        } => match tag_name.as_str() {
            "style" | "base" => true,
            "link" => attributes.iter().any(|(k, v)| {
                k == "rel"
                    && v.split_ascii_whitespace()
                        .any(|word| word.eq_ignore_ascii_case("stylesheet"))
            }),
            _ => false,
        },
        _ => false,
    }
}

fn count_sheet_elements(doc: &Document) -> usize {
    let mut count = 0;
    let mut stack = vec![doc.root()];
    while let Some(node) = stack.pop() {
        stack.extend(doc.children(node));
        if is_sheet_element(doc, node) {
            count += 1;
        }
    }
    count
}

/// Whether `node`'s subtree holds a stylesheet source.
fn subtree_has_sheet_element(doc: &Document, node: NodeId) -> bool {
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if is_sheet_element(doc, n) {
            return true;
        }
        stack.extend(doc.children(n));
    }
    false
}

/// For the profile output: which kind of change made the sheets count as changed, and the start
/// of the text of the style element involved.
fn describe_sheet_change(doc: &Document, changes: &Changes, epoch: &StyleEpoch) -> String {
    let text_of = |node: NodeId| {
        let mut text = String::new();
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if let blueice_dom::NodeData::Text { data } = doc.data(n) {
                text.push_str(data);
            }
            let mut kids: Vec<_> = doc.children(n).collect();
            kids.reverse();
            stack.extend(kids);
        }
        let head: String = text.chars().take(60).collect();
        format!("{} bytes: {}", text.len(), head.replace('\n', " "))
    };
    if let Some(&n) = changes
        .touched
        .keys()
        .find(|&&n| doc.contains(n) && (is_sheet_element(doc, n) || doc.parent(n).is_some_and(|p| is_sheet_element(doc, p))))
    {
        let style = if is_sheet_element(doc, n) { n } else { doc.parent(n).unwrap_or(n) };
        return format!("a style element was edited, {}", text_of(style));
    }
    if let Some(&(parent, child)) = changes
        .attached
        .iter()
        .find(|&&(parent, child)| doc.contains(child) && (is_sheet_element(doc, parent) || subtree_has_sheet_element(doc, child)))
    {
        let style = if is_sheet_element(doc, parent) { parent } else { child };
        return format!("a style element or its content was added, {}", text_of(style));
    }
    if changes.detached.iter().any(|d| doc.contains(d.parent) && is_sheet_element(doc, d.parent)) {
        return "content was removed from a style element".to_string();
    }
    format!(
        "the number of style/link elements went from {} to {}",
        epoch.sheet_elements,
        count_sheet_elements(doc)
    )
}

/// Whether something that feeds the author stylesheets changed.
fn sheets_changed(doc: &Document, changes: &Changes, epoch: &StyleEpoch) -> bool {
    let inside_style = |node: NodeId| doc.parent(node).is_some_and(|p| is_sheet_element(doc, p));
    changes.touched.keys().any(|&n| {
        doc.contains(n) && (is_sheet_element(doc, n) || inside_style(n))
    }) || changes
        .attached
        .iter()
        .any(|&(parent, child)| {
            doc.contains(child)
                && (is_sheet_element(doc, parent) || subtree_has_sheet_element(doc, child))
        })
        || changes
            .detached
            .iter()
            .any(|d| doc.contains(d.parent) && is_sheet_element(doc, d.parent))
        // A removed subtree can no longer be searched, but the count shows it went.
        || count_sheet_elements(doc) != epoch.sheet_elements
}

/// The rules that were added or taken away between two rule lists, in a pass over both: where
/// they differ, the next place the old rule turns up in the new list tells what was inserted
/// before it, and a rule found nowhere near is one that went. `None` when more than `most`
/// differ. Rules left in place keep their order, so the elements a changed rule does not reach
/// cascade exactly as before wherever the new sheets sit.
fn changed_rules<'a>(
    old: &'a [blueice_css::Rule],
    new: &'a [blueice_css::Rule],
    most: usize,
) -> Option<Vec<&'a blueice_css::Rule>> {
    /// How far ahead in the new list an old rule is looked for.
    const WINDOW: usize = 4096;
    let mut changed = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            i += 1;
            j += 1;
            continue;
        }
        let ahead = &new[j + 1..new.len().min(j + 1 + WINDOW)];
        match ahead.iter().position(|rule| *rule == old[i]) {
            Some(skipped) => {
                changed.extend(&new[j..j + 1 + skipped]);
                j += 1 + skipped;
            }
            None => {
                changed.push(&old[i]);
                i += 1;
            }
        }
        if changed.len() > most {
            return None;
        }
    }
    changed.extend(&old[i..]);
    changed.extend(&new[j..]);
    (changed.len() <= most).then_some(changed)
}

impl Page {
    /// Records what a full cascade was computed from, and starts a fresh change period.
    pub(super) fn note_full_cascade(
        &mut self,
        author: Vec<blueice_css::Rule>,
        media: MediaEnvironment,
    ) {
        self.style_epoch = Some(StyleEpoch {
            media,
            sheet_elements: count_sheet_elements(&self.doc),
        });
        self.author_rules = author;
        self.doc.take_changes();
        self.restyle_counts.1 += 1;
    }

    /// Handles a stylesheet change that touched only a few rules (a script put a `<style>` into
    /// the page, as a picture's own `.st0 { fill: ... }` sheet, or took one away, or edited
    /// one): the rules the old and new lists do not share (see [`changed_rules`]; they may sit
    /// anywhere in the list) are the only ones that can make an element's style differ. The elements those match are restyled, with their subtrees, against the complete
    /// new list in its proper order, as if their attributes had changed -- exact, wherever the
    /// changed rules sit. `false` when the changed rules are many or reach too much of the page
    /// to be worth it; nothing is changed then.
    fn restyle_for_appended_rules(&mut self, changes: &Changes, epoch: StyleEpoch) -> bool {
        /// More new rules than this, or matching more than half the page's elements, and a full
        /// cascade is as cheap.
        const MOST_RULES: usize = 64;
        let environment = self.media_environment();
        let author = crate::stylesheet::extract_inline_stylesheets_with_linked(
            &self.doc,
            &environment,
            &self.linked_styles,
        );
        let old = &self.author_rules;
        let Some(appended) = changed_rules(old, &author, MOST_RULES) else {
            if std::env::var("BI_PROFILE_LAYOUT").is_ok() {
                eprintln!(
                    "RESTYLE not by rules: more than {MOST_RULES} changed rules (old {}, new {})",
                    old.len(),
                    author.len()
                );
            }
            return false;
        };
        let mut matched: Vec<NodeId> = Vec::new();
        let mut elements = 0usize;
        let mut stack = vec![self.doc.root()];
        while let Some(node) = stack.pop() {
            stack.extend(self.doc.children(node));
            // A shadow tree's nodes are searched too: a document rule can match inside one, and
            // such a match sends the restyle to a full cascade.
            if let Some(shadow) = self.doc.shadow_root(node) {
                stack.push(shadow);
            }
            if !matches!(self.doc.data(node), NodeData::Element { .. }) {
                continue;
            }
            elements += 1;
            let hit = appended.iter().any(|rule| {
                rule.selectors.iter().any(|selector| {
                    blueice_css::matches(&self.doc, node, selector)
                        || blueice_css::matches_pseudo_element(&self.doc, node, selector)
                })
            });
            if hit {
                matched.push(node);
            }
        }
        if matched.len() * 2 > elements {
            return false;
        }
        // Each matched element looks, to the incremental cascade, like one whose attributes
        // changed: it and everything below it is recomputed.
        let mut effective = changes.clone();
        for node in matched {
            effective
                .touched
                .entry(node)
                .or_insert_with(|| blueice_dom::Before::Element {
                    tag_name: match self.doc.data(node) {
                        NodeData::Element { tag_name, .. } => tag_name.clone(),
                        _ => String::new(),
                    },
                    attributes: vec![("data-appended-rules".to_string(), String::new())],
                });
        }
        let mut style_changed = Vec::new();
        let done = cascade_incremental_report_with_containers(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &author)],
            self.viewport_width,
            self.viewport_height,
            &self.container_sizes,
            &mut self.styles,
            &effective,
            &mut style_changed,
        );
        if !done {
            return false;
        }
        self.author_rules = author;
        self.style_epoch = Some(StyleEpoch {
            media: epoch.media,
            sheet_elements: count_sheet_elements(&self.doc),
        });
        self.restyle_counts.0 += 1;
        self.rebuild_images();
        self.layout_cache
            .invalidate(&self.doc, &effective, &style_changed);
        true
    }

    /// Updates `self.styles` from the document's change record. `false` means a full
    /// cascade is needed (and `self.styles` is as it was).
    pub(super) fn restyle_incrementally(&mut self) -> bool {
        let Some(epoch) = self.style_epoch else {
            return false;
        };
        let changes = self.doc.take_changes();
        let profile = std::env::var("BI_PROFILE_LAYOUT").is_ok();
        if self.media_environment() != epoch.media {
            if profile {
                eprintln!("RESTYLE full: the media environment changed");
            }
            return false;
        }
        if sheets_changed(&self.doc, &changes, &epoch) {
            if self.restyle_for_appended_rules(&changes, epoch) {
                return true;
            }
            if profile {
                eprintln!("RESTYLE appended-rules path declined (shadow roots {})", self.doc.has_shadow_roots());
            }
            if profile {
                eprintln!(
                    "RESTYLE full: a stylesheet source changed ({})",
                    describe_sheet_change(&self.doc, &changes, &epoch)
                );
            }
            return false;
        }
        let mut style_changed = Vec::new();
        let done = cascade_incremental_report_with_containers(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &self.author_rules)],
            self.viewport_width,
            self.viewport_height,
            &self.container_sizes,
            &mut self.styles,
            &changes,
            &mut style_changed,
        );
        if done {
            self.restyle_counts.0 += 1;
            self.rebuild_images();
            // Layout reuses what it computed for every box these changes cannot have
            // touched.
            self.layout_cache
                .invalidate(&self.doc, &changes, &style_changed);
        } else if profile {
            eprintln!(
                "RESTYLE full: the incremental path declined ({} touched, {} attached, {} detached, overflow {}, shadow roots {})",
                changes.touched.len(),
                changes.attached.len(),
                changes.detached.len(),
                changes.overflow,
                self.doc.has_shadow_roots()
            );
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_css::{cascade_with_viewport, ComputedStyle};
    use std::collections::HashMap;

    fn page(html: &str) -> Page {
        let mut page = Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.load_html_str(html, Some("https://example.com/p".to_string()));
        page
    }

    fn fresh(page: &Page) -> HashMap<NodeId, ComputedStyle> {
        cascade_with_viewport(
            &page.doc,
            &[(Origin::Ua, &page.ua), (Origin::Author, &page.author_rules)],
            page.viewport_width,
            page.viewport_height,
        )
    }

    #[test]
    fn a_script_changing_one_elements_style_restyles_incrementally_and_matches_a_full_cascade() {
        let mut page = page(
            "<style>p { color: blue } .hot { color: red }</style>\
             <body><p id=a>one</p><p id=b>two</p><script>\
             window.change = () => {\
               document.getElementById('a').style.width = '50px';\
               document.getElementById('b').className = 'hot';\
               const p = document.createElement('p'); p.textContent = 'three'; document.body.appendChild(p);\
             };</script></body>",
        );
        let (incremental, full) = page.restyle_counts;
        page.web_eval("window.change()");
        page.settle();
        assert_eq!(
            page.styles,
            fresh(&page),
            "same styles as a full cascade would give"
        );
        assert!(
            page.restyle_counts.0 > incremental,
            "the change was restyled incrementally"
        );
        assert_eq!(page.restyle_counts.1, full, "no full cascade was needed");
    }

    #[test]
    fn a_style_a_script_puts_in_a_shadow_root_styles_the_host_and_its_tree() {
        // Lit's `:host { display: contents }`: the shadow tree is not under the document root,
        // yet its sheet counts.
        let mut page = page(
            "<body><my-el id=h><b id=k>x</b></my-el><script></script></body>",
        );
        page.web_eval(
            "const r = document.getElementById('h').attachShadow({mode: 'open'});\
             const s = document.createElement('style'); s.textContent = ':host { display: contents } slot { color: red }';\
             r.appendChild(s); r.appendChild(document.createElement('slot'));",
        );
        page.settle();
        let host = page.styles.keys().copied().find(|n| {
            matches!(page.doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "my-el")
        });
        let host = host.expect("the host");
        assert_eq!(page.styles[&host].display, "contents");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn an_svg_a_script_puts_in_a_shadow_root_keeps_its_attribute_size() {
        let mut page = Page::new(1280.0, 800.0);
        page.set_web_scripts(true);
        page.load_html_str(
            "<body><my-btn></my-btn><script>\
             const r = document.querySelector('my-btn').attachShadow({mode: 'open'});\
             r.innerHTML = '<style>.b{display:flex;align-items:center;border:1px solid;padding:4px;width:80px;justify-content:space-between}.b:before{content:\"\";height:18px;width:15px}</style><button class=b><svg width=24 height=24 viewBox=\"0 0 24 24\"><path d=\"m1 1\"/></svg></button>';\
             </script></body>",
            Some("https://example.com/p".to_string()),
        );
        page.settle();
        fn find<'a>(f: &'a blueice_layout::Fragment, doc: &blueice_dom::Document, tag: &str) -> Option<&'a blueice_layout::Fragment> {
            if f.node.is_some_and(|n| matches!(doc.data(n), NodeData::Element { tag_name, .. } if tag_name == tag)) {
                return Some(f);
            }
            f.children.iter().find_map(|c| find(c, doc, tag))
        }
        let svg = find(&page.fragment, &page.doc, "svg").expect("the svg is laid out");
        let button = find(&page.fragment, &page.doc, "button").expect("the button is laid out");
        assert_eq!((svg.width, svg.height, button.height), (24.0, 24.0, 34.0));
    }

    #[test]
    fn hiding_a_slot_later_hides_what_is_assigned_to_it() {
        let mut page = page("<body><my-el id=h><p id=k>x</p></my-el><script></script></body>");
        page.web_eval(
            "const r = document.getElementById('h').attachShadow({mode: 'open'}); r.innerHTML = '<slot id=s></slot>';",
        );
        page.settle();
        page.web_eval("r2 = document.getElementById('h').shadowRoot.getElementById('s'); r2.setAttribute('hidden', '')");
        page.settle();
        let slot = page.styles.keys().copied().find(|n| {
            matches!(page.doc.data(*n), NodeData::Element { tag_name, .. } if tag_name == "slot")
        });
        assert_eq!(page.styles[&slot.expect("the slot")].display, "none");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn rules_appended_after_all_the_old_ones_restyle_only_what_they_match() {
        // A logo's own `<style>` (`.st0 { fill: ... }`) arrives with the picture.
        let mut page = page(
            "<style>p { color: blue } svg { display: block }</style>\
             <body><p id=a>one</p><svg id=s><g class=st0 id=g></g><g id=h></g></svg><p id=b>two</p></body>",
        );
        let (incremental, full) = page.restyle_counts;
        page.web_eval(
            "const s = document.createElement('style'); s.textContent = '.st0 { fill: #ff0000 } p#b { color: red }'; \
             document.getElementById('s').appendChild(s);",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page), "same styles as a full cascade");
        assert!(page.restyle_counts.0 > incremental, "restyled incrementally");
        assert_eq!(page.restyle_counts.1, full, "no full cascade");
        let red = page
            .styles
            .values()
            .filter(|s| s.color == blueice_css::Color::Rgba(255, 0, 0, 255))
            .count();
        assert_eq!(red, 1, "p#b picked up its new colour");
        assert!(
            page.styles.values().any(|s| matches!(s.other.get("fill"),
                Some(blueice_css::Value::Color(blueice_css::Color::Rgba(255, 0, 0, 255))))),
            "and .st0 its fill"
        );
        // And again: each appended sheet is handled the same way.
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = '#h { fill: #00ff00 }'; document.body.appendChild(s); }",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        assert_eq!(page.restyle_counts.1, full, "still no full cascade");
    }

    #[test]
    fn rules_that_land_between_old_ones_are_ordered_correctly_without_a_full_cascade() {
        let mut page = page(
            "<style>p { color: blue }</style><body><p id=a>one</p><p id=b class=x>two</p></body>",
        );
        let (incremental, full) = page.restyle_counts;
        // In front of the old sheet: its rule loses a tie to the old one, as in a full cascade.
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = 'p { color: red } .x { margin-top: 7px }'; \
             document.head.insertBefore(s, document.head.firstChild); }",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        assert!(page.restyle_counts.0 > incremental, "incremental");
        assert_eq!(page.restyle_counts.1, full, "no full cascade");
        let blue = page
            .styles
            .values()
            .filter(|s| s.color == blueice_css::Color::Rgba(0, 0, 255, 255))
            .count();
        assert!(blue >= 2, "the old, later rule still wins the tie: {blue}");
    }

    #[test]
    fn a_style_element_inside_an_svg_feeds_the_page_no_rules_and_costs_no_full_cascade() {
        let mut page = page("<style>p { color: blue }</style><body><p id=a>one</p><svg id=s><g class=st0></g></svg></body>");
        let (incremental, full) = page.restyle_counts;
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = '.st0 { fill: #f00 }'; \
             document.getElementById('s').appendChild(s); }",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        assert!(page.restyle_counts.0 > incremental);
        assert_eq!(page.restyle_counts.1, full);
    }

    #[test]
    fn a_removed_or_edited_sheet_restyles_only_what_its_rules_matched() {
        let mut page = page(
            "<style>p { color: blue } .k { margin-left: 3px }</style>\
             <body><p id=a>one</p><svg id=s><style id=t>.st0 { fill: #ff0000 }</style><g class=st0></g></svg></body>",
        );
        let (_, full) = page.restyle_counts;
        page.web_eval("document.getElementById('t').remove();");
        page.settle();
        assert_eq!(page.styles, fresh(&page), "a removed sheet");
        assert_eq!(page.restyle_counts.1, full, "without a full cascade");
        page.web_eval("document.querySelector('style').textContent = 'p { color: green } .k { margin-left: 3px }';");
        page.settle();
        assert_eq!(page.styles, fresh(&page), "an edited sheet");
        assert_eq!(page.restyle_counts.1, full, "without a full cascade");
        let green = page
            .styles
            .values()
            .any(|s| s.color == blueice_css::Color::Rgba(0, 128, 0, 255));
        assert!(green, "the edit applies");
    }

    #[test]
    fn a_stylesheet_that_matches_most_of_the_page_takes_a_full_cascade() {
        let mut page = page("<style>p { color: blue }</style><body><p id=a>one</p></body>");
        let (_, full) = page.restyle_counts;
        // Matching every element is not worth reasoning about.
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = '* { margin: 1px }'; document.body.appendChild(s); }",
        );
        page.settle();
        assert!(page.restyle_counts.1 > full, "matches everything");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn a_new_stylesheet_matching_the_page_or_a_resize_agrees_with_a_full_cascade() {
        let mut page = page("<style>p { color: blue }</style><body><p id=a>one</p></body>");
        // Removing a sheet changes old rules; the styles still come out as a full cascade's.
        page.web_eval("document.querySelector('style').remove();");
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = 'p { color: red }'; document.head.appendChild(s); }",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        let red = page
            .styles
            .values()
            .any(|s| s.color == blueice_css::Color::Rgba(255, 0, 0, 255));
        assert!(red, "and its rule applies");
        let full = page.restyle_counts.1;
        page.resize(500.0, 300.0);
        assert!(page.restyle_counts.1 > full, "a resize is a full cascade");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn inserting_a_preload_or_a_stylesheet_link_agrees_with_a_full_cascade() {
        let mut page = page("<body><p>x</p></body>");
        let (incremental, full) = page.restyle_counts;
        page.web_eval(
            "const l = document.createElement('link'); l.rel = 'preload'; l.href = '/a.js'; document.head.appendChild(l);\
             document.body.className = 'x';",
        );
        page.settle();
        assert!(
            page.restyle_counts.0 > incremental,
            "a preload link feeds no rule"
        );
        assert_eq!(page.restyle_counts.1, full);
        let full = page.restyle_counts.1;
        page.web_eval(
            "const sheet = document.createElement('link'); sheet.rel = 'stylesheet'; sheet.href = '/a.css'; document.head.appendChild(sheet);",
        );
        page.settle();
        // A link inserted after load is not fetched, so it adds no rule: the styles are what a
        // full cascade gives whichever way they were reached.
        assert_eq!(page.styles, fresh(&page), "a stylesheet link");
        let _ = full;
        // Turning a preload into a stylesheet changes what feeds the rules too.
        page.web_eval("document.querySelector('link[rel=preload]').rel = 'stylesheet';");
        page.settle();
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn a_page_with_a_shadow_root_still_restyles_changes_away_from_it_incrementally() {
        // github.com attaches shadow roots as it boots; one anywhere used to make every later
        // restyle a full cascade (3 s each there).
        let mut page = page("<body><div id=host></div><p id=a>x</p><p id=b>y</p></body>");
        page.web_eval(
            "const root = document.getElementById('host').attachShadow({mode:'open'}); root.innerHTML = '<span>in</span>';",
        );
        page.settle();
        assert_eq!(page.styles, fresh(&page));
        let (incremental, _full) = page.restyle_counts;
        page.web_eval("document.getElementById('a').className = 'k';");
        page.settle();
        assert!(page.restyle_counts.0 > incremental, "a class change away from the shadow tree");
        assert_eq!(page.styles, fresh(&page));
        let (incremental, _full) = page.restyle_counts;
        page.web_eval(
            "{ const s = document.createElement('style'); s.textContent = 'p.k { color: red }'; document.head.appendChild(s); }",
        );
        page.settle();
        assert!(page.restyle_counts.0 > incremental, "a new rule that matches nothing in a shadow tree");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn stylesheets_added_in_two_places_restyle_only_what_their_rules_reach() {
        // turbo (github.com) puts a `<style>` first in `<head>` and another goes in later: a
        // prefix-and-suffix comparison of the old and new rules called everything between
        // them changed, and the restyle was a full cascade (3 s there).
        let mut html = String::from("<head><style>b { color: blue }</style>");
        for n in 0..100 {
            html.push_str(&format!("<style>.r{n} {{ margin-left: {n}px }}</style>"));
        }
        html.push_str("</head><body><p id=a class=r5>x</p><i>y</i></body>");
        let mut page = page(&html);
        assert_eq!(page.styles, fresh(&page));
        let (incremental, _full) = page.restyle_counts;
        page.web_eval(
            "{ const first = document.createElement('style'); first.textContent = 'p { color: red }'; \
               document.head.insertBefore(first, document.head.firstChild); \
               const last = document.createElement('style'); last.textContent = 'i { color: green }'; \
               document.head.appendChild(last); }",
        );
        page.settle();
        assert!(page.restyle_counts.0 > incremental, "restyled by the rules the two sheets added");
        assert_eq!(page.styles, fresh(&page));
    }

    #[test]
    fn removing_a_subtree_drops_its_styles() {
        let mut page = page("<body><div id=d><p>x</p><p>y</p></div><span>z</span></body>");
        let before = page.styles.len();
        page.web_eval("document.getElementById('d').remove();");
        page.settle();
        assert!(page.styles.len() < before);
        assert_eq!(page.styles, fresh(&page));
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    const DOCUMENT: &str = "<style>\
        .a { float: left; width: 30px; height: 10px } .b { display: inline-block; width: 20px } \
        .c { display: flex } .c > * { flex: 1 } .d { position: relative; top: 2px } \
        .e { position: absolute; left: 3px; top: 4px; width: 5px; height: 5px } \
        .f { padding: 3px; border: 1px solid red; margin: 4px } p { margin: 6px 0 } \
        li { margin-bottom: 2px } .g { width: 50% } .h { overflow: hidden; height: 20px } \
        .i { transform: translate(5px, 5px) } .j { display: none } ul, ol { padding-left: 20px }\
        </style><body>\
        <div id=n0 class=f><p id=n1>alpha beta gamma</p><p id=n2>delta epsilon</p></div>\
        <ol id=n3><li id=n4>one</li><li id=n5>two</li><li id=n6>three</li></ol>\
        <div id=n7 class=c><span id=n8>x</span><span id=n9>y</span></div>\
        <section id=n10><ul id=n11><li id=n12>a</li><li id=n13>b</li></ul><p id=n14>tail text</p></section>\
        </body>";

    /// One random edit as script, guarded so that it does nothing if its target was removed.
    fn edit(rng: &mut Rng, ids: &mut u32) -> String {
        const CLASSES: &[&str] = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", ""];
        const STYLES: &[&str] = &[
            "width: 40px",
            "height: 12px",
            "display: none",
            "float: right",
            "padding: 4px",
            "",
        ];
        const TAGS: &[&str] = &["div", "p", "span", "li", "section"];
        let target = format!("document.getElementById('n{}')", rng.below(*ids as usize));
        let body = match rng.below(9) {
            0 => format!("t.className = '{}';", CLASSES[rng.below(CLASSES.len())]),
            1 => format!("t.setAttribute('style', '{}');", STYLES[rng.below(STYLES.len())]),
            2 => format!("t.textContent = 'w{} {}';", rng.below(100), "word ".repeat(rng.below(6))),
            3 => {
                *ids += 1;
                format!(
                    "var e = document.createElement('{}'); e.id = 'n{}'; e.className = '{}'; e.textContent = 'new {}'; t.appendChild(e);",
                    TAGS[rng.below(TAGS.len())],
                    *ids - 1,
                    CLASSES[rng.below(CLASSES.len())],
                    rng.below(50)
                )
            }
            4 => {
                *ids += 1;
                format!(
                    "var e = document.createElement('li'); e.id = 'n{}'; e.textContent = 'item'; \
                     t.insertBefore(e, t.firstChild);",
                    *ids - 1
                )
            }
            5 => "if (t.parentNode && t.id != 'n0') t.remove();".to_string(),
            6 => format!("t.setAttribute('start', '{}');", 1 + rng.below(9)),
            7 => format!("t.setAttribute('value', '{}');", 1 + rng.below(9)),
            _ => format!(
                "var o = document.getElementById('n{}'); if (o && t.parentNode && !t.contains(o) && o.nodeType == 1) o.appendChild(t);",
                rng.below(*ids as usize)
            ),
        };
        format!("(function(){{ var t = {target}; if (t) {{ {body} }} }})();")
    }

    #[test]
    fn a_page_relaid_out_through_the_cache_equals_a_fresh_layout_after_random_script_edits() {
        let mut reused = 0;
        let mut passes = 0;
        for seed in 1..=6u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut page = page(DOCUMENT);
            let mut ids = 15u32;
            for step in 0..15 {
                let mut script = String::new();
                for _ in 0..1 + rng.below(3) {
                    script.push_str(&edit(&mut rng, &mut ids));
                }
                page.web_eval(&script);
                page.settle();
                let fresh_layout = blueice_layout::layout(
                    &page.doc,
                    page.doc.root(),
                    &page.styles,
                    blueice_layout::Constraints {
                        available_width: page.viewport_width,
                        viewport_height: page.viewport_height,
                    },
                );
                assert!(
                    page.fragment == fresh_layout,
                    "seed {seed} step {step}: the page's fragment tree differs from a fresh layout\n{script}"
                );
                assert_eq!(page.styles, fresh(&page), "seed {seed} step {step}: styles");
                passes += 1;
            }
            reused += page.layout_reuse;
        }
        assert!(
            reused * 2 > passes,
            "the cache must actually be reused ({reused} of {passes} passes)"
        );
    }
}
