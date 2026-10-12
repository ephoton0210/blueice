// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native previews describe the current Core pixels and public selection.

use super::*;
use blueice_ipc::input::{NativeDragImage, PointerPoint};
use blueice_ipc::Bounds;

impl Page {
    pub(crate) fn native_drag_image(&self, id: u64, source: u64) -> Option<NativeDragImage> {
        let drag = self
            .native_drag_source
            .as_ref()
            .filter(|drag| drag.id == id)?;
        let (regions, requested) = if let Some(image) = &drag.image {
            let node = NodeId::from_u64(image.node_id);
            if !self.doc.contains(node) || !matches!(self.doc.data(node), NodeData::Element { .. })
            {
                return None;
            }
            (
                vec![self.public_drag_image_bounds(node)?],
                Some((image.x, image.y)),
            )
        } else {
            let input = self.native_text_input_state(source);
            let regions = if let Some(control) = input.focused.filter(|control| {
                control.node_id == drag.target.as_u64()
                    && !control.protected
                    && control.selection.length > 0
            }) {
                control.selection_rects
            } else if let Some(document) = input
                .document
                .filter(|document| document.active && document.selection.length > 0)
            {
                document.selection_rects
            } else {
                vec![self.public_drag_image_bounds(drag.target)?]
            };
            (regions, None)
        };
        if regions.is_empty() || regions.len() > 256 {
            return None;
        }
        let regions: Vec<_> = regions
            .into_iter()
            .map(|mut rect| {
                rect.y -= self.scroll_y;
                rect
            })
            .collect();
        if regions.iter().any(|rect| {
            ![rect.x, rect.y, rect.width, rect.height]
                .iter()
                .all(|v| v.is_finite())
                || rect.width <= 0.0
                || rect.height <= 0.0
        }) {
            return None;
        }
        let left = regions.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
        let top = regions.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
        let hotspot = requested.map_or(
            PointerPoint {
                x: drag.point.x - left,
                y: drag.point.y - top,
            },
            |(x, y)| PointerPoint {
                x: x as f64,
                y: y as f64,
            },
        );
        Some(NativeDragImage { regions, hotspot })
    }

    fn public_drag_image_bounds(&self, node: NodeId) -> Option<Bounds> {
        let bounds = find_fragment_bounds(&self.fragment, node, 0.0, 0.0)?;
        let mut pending = vec![node];
        let mut visits = 0;
        while let Some(next) = pending.pop() {
            visits += 1;
            if visits > 65_536 {
                return None;
            }
            // A script-selected container cannot turn protected rendered
            // descendants into an exported bitmap. Hidden descendants have
            // no painted bounds and contribute no pixels.
            if find_fragment_bounds(&self.fragment, next, 0.0, 0.0).is_some()
                && !self.accessibility_safe_content(next)
            {
                return None;
            }
            pending.extend(self.doc.children(next));
        }
        Some(bounds)
    }
}
