// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluejs::{HostDragEventData, HostDragEventKind, HostDragItem, HostFileData};
use blueice_ipc::input::{NativeDragItem, NativeDragOperation};
use blueice_ipc::page_host::{PageHostDragEvent, PageHostDragKind, PageHostDragOutcome};

impl BlueJsChildHost {
    pub(super) fn dispatch_drag(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        nodes: Vec<u64>,
        event: PageHostDragEvent,
    ) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return stale_document();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        if self
            .script_dom_capability
            .as_ref()
            .map(ScriptDomCapability::profile)
            != Some(PageDomProfile::File)
            || nodes.is_empty()
            || nodes.len() > 256
            || !event.valid()
            || nodes.iter().copied().collect::<BTreeSet<_>>().len() != nodes.len()
        {
            return invalid_request();
        }
        let Some(family) = document.click_event_family else {
            return host_failure();
        };
        let path = nodes
            .into_iter()
            .map(|node| HostObjectKey::new(tab_id, document_generation, node))
            .collect::<Vec<_>>();
        let point = event.point();
        let mut input = HostDragEventData::new(
            match event.kind {
                PageHostDragKind::DragStart => HostDragEventKind::DragStart,
                PageHostDragKind::Drag => HostDragEventKind::Drag,
                PageHostDragKind::DragEnter => HostDragEventKind::DragEnter,
                PageHostDragKind::DragOver => HostDragEventKind::DragOver,
                PageHostDragKind::DragLeave => HostDragEventKind::DragLeave,
                PageHostDragKind::Drop => HostDragEventKind::Drop,
                PageHostDragKind::DragEnd => HostDragEventKind::DragEnd,
            },
            point.x,
            point.y,
        );
        input.types = event.types;
        input.screen_x = f64::from_bits(event.screen_x_bits);
        input.screen_y = f64::from_bits(event.screen_y_bits);
        input.page_x = f64::from_bits(event.page_x_bits);
        input.page_y = f64::from_bits(event.page_y_bits);
        input.offset_x = f64::from_bits(event.offset_x_bits);
        input.offset_y = f64::from_bits(event.offset_y_bits);
        input.related_target = event
            .related_target
            .map(|node| HostObjectKey::new(tab_id, document_generation, node));
        input.buttons = event.buttons;
        input.alt_key = event.modifiers.alt;
        input.ctrl_key = event.modifiers.control;
        input.meta_key = event.modifiers.meta;
        input.shift_key = event.modifiers.shift;
        input.caps_lock = event.modifiers.caps_lock;
        input.items = event
            .items
            .into_iter()
            .map(|item| match item {
                NativeDragItem::Text { format, data } => HostDragItem::Text { format, data },
                NativeDragItem::File(file) => HostDragItem::File(HostFileData {
                    name: file.name,
                    media_type: file.media_type,
                    last_modified: file.last_modified,
                    bytes: file.bytes,
                }),
            })
            .collect();
        input.drop_effect = event.operation.as_str().into();
        input.effect_allowed = event.effect_allowed;
        let result = match self
            .runtime
            .dispatch_host_drag_event(tab_id, family, &path, &input)
        {
            Ok(result) => result,
            Err(_) => return host_failure(),
        };
        let Some(operation) = NativeDragOperation::from_effect(&result.drop_effect) else {
            return host_failure();
        };
        let outcome = PageHostDragOutcome {
            default_prevented: result.default_prevented,
            operation,
            effect_allowed: result.effect_allowed,
            image: result
                .image
                .map(|image| blueice_ipc::page_host::PageHostDragImage {
                    node_id: image.element.object(),
                    x: image.x,
                    y: image.y,
                }),
            items: result
                .items
                .into_iter()
                .map(|item| match item {
                    HostDragItem::Text { format, data } => NativeDragItem::Text { format, data },
                    HostDragItem::File(file) => {
                        NativeDragItem::File(blueice_ipc::file_input::FileData {
                            name: file.name,
                            media_type: file.media_type,
                            last_modified: file.last_modified,
                            bytes: file.bytes,
                        })
                    }
                })
                .collect(),
        };
        if !outcome.valid(event.kind) {
            return host_failure();
        }
        PageHostReply::DragDispatched {
            tab_id,
            document_generation,
            kind: event.kind,
            outcome,
        }
    }

    pub(super) fn flush_drag_tasks(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return stale_document();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        // Scheduling an existing realm's jobs does not install a capability.
        // All profiles retain their own globals and exact-document callbacks.
        let timer_epoch = document.timer_epoch;
        let now_ms = || u64::try_from(timer_epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        if self.runtime.flush_drag_tasks(tab_id).is_err() {
            return host_failure();
        }
        if self.runtime.run_due_timers(tab_id, now_ms()).is_err() {
            return host_failure();
        }
        let next_wake_in_ms = match self.runtime.next_timer_due(tab_id) {
            Ok(due) => due.map(|due| due.saturating_sub(now_ms())),
            Err(_) => return host_failure(),
        };
        let scheduling_payload_bytes = match self.runtime.host_scheduling_usage(tab_id) {
            Ok(usage) => u64::try_from(usage.payload_bytes).unwrap_or(u64::MAX),
            Err(_) => return host_failure(),
        };
        PageHostReply::DragTasksFlushed {
            tab_id,
            document_generation,
            next_wake_in_ms,
            scheduling_payload_bytes,
            animation_frame_pending: self
                .runtime
                .animation_frame_pending(tab_id)
                .unwrap_or(false),
        }
    }

    /// The host supplies one document-clock sample for the whole callback
    /// snapshot. Neither browser tasks nor due timers execute in this entry.
    pub(super) fn run_animation_frame(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> PageHostReply {
        let Some(document) = self.documents.get(&tab_id) else {
            return stale_document();
        };
        if document.generation != document_generation {
            return stale_document();
        }
        let epoch = document.timer_epoch;
        if self
            .runtime
            .run_animation_frame(tab_id, epoch.elapsed().as_secs_f64() * 1000.0)
            .is_err()
        {
            return host_failure();
        }
        let now = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (Ok(due), Ok(animation_frame_pending)) = (
            self.runtime.next_timer_due(tab_id),
            self.runtime.animation_frame_pending(tab_id),
        ) else {
            return host_failure();
        };
        let scheduling_payload_bytes = match self.runtime.host_scheduling_usage(tab_id) {
            Ok(usage) => u64::try_from(usage.payload_bytes).unwrap_or(u64::MAX),
            Err(_) => return host_failure(),
        };
        PageHostReply::AnimationFrameRendered {
            tab_id,
            document_generation,
            next_wake_in_ms: due.map(|due| due.saturating_sub(now)),
            animation_frame_pending,
            scheduling_payload_bytes,
        }
    }
}
