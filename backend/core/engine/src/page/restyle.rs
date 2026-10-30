// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Restyling a page after a change by redoing only what the document says changed
//! (`blueice_css::cascade_incremental`), falling back to a full cascade whenever the
//! inputs to the rules themselves may have changed.

use super::Page;
use blueice_css::{cascade_incremental, MediaEnvironment, Origin};
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
            if profile {
                eprintln!("RESTYLE full: a stylesheet source changed");
            }
            return false;
        }
        let done = cascade_incremental(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &self.author_rules)],
            self.viewport_width,
            self.viewport_height,
            &mut self.styles,
            &changes,
        );
        if done {
            self.restyle_counts.0 += 1;
            self.rebuild_images();
        } else if profile {
            eprintln!(
                "RESTYLE full: the incremental path declined ({} touched, {} attached, {} detached, overflow {})",
                changes.touched.len(),
                changes.attached.len(),
                changes.detached.len(),
                changes.overflow
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
    fn a_new_stylesheet_or_a_resize_takes_a_full_cascade() {
        let mut page = page("<body><p id=a>one</p></body>");
        let (_, full) = page.restyle_counts;
        page.web_eval(
            "const s = document.createElement('style'); s.textContent = 'p { color: red }'; document.head.appendChild(s);",
        );
        page.settle();
        assert!(
            page.restyle_counts.1 > full,
            "a new <style> is a full cascade"
        );
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
    fn inserting_a_preload_link_is_incremental_but_a_stylesheet_link_is_not() {
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
        assert!(page.restyle_counts.1 > full, "a stylesheet link might");
        // Turning a preload into a stylesheet changes what feeds the rules too.
        let full = page.restyle_counts.1;
        page.web_eval("document.querySelector('link[rel=preload]').rel = 'stylesheet';");
        page.settle();
        assert!(page.restyle_counts.1 > full);
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
}
