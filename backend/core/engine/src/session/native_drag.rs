// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Serial native drag transactions. Script runs before defaults; every child
//! round trip is followed by a live document check before core can mutate it.

use super::*;
use crate::page::native_drag::{effect_allowed, restrict_operations, DragDestination};
use blueice_ipc::input::{
    NativeDragAction, NativeDragItem, NativeDragMouse, NativeDragOperation as Operation,
    NativeDragState, PointerContext, PointerPoint,
};
use blueice_ipc::page_host::{PageHostDragEvent, PageHostDragKind as Kind, PageHostDragOutcome};

// Keep owner/document fences and script services explicit, as in the other
// serial session handlers.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_native_drag(
    tabs: &mut TabManager,
    tab: TabId,
    source: u64,
    context: &PointerContext,
    frame: u64,
    action: NativeDragAction,
    mouse: Option<NativeDragMouse>,
    executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    requests: Option<&ScriptRequestReceiver>,
) -> Result<NativeDragState, String> {
    validate_document(tabs, tab, source, context)?;
    if mouse.is_some_and(|mouse| !mouse.valid()) {
        return Err("Invalid native drag mouse snapshot".into());
    }
    let mouse_point = match &action {
        NativeDragAction::Enter { point, .. }
        | NativeDragAction::Drop { point, .. }
        | NativeDragAction::SourceMove { point, .. } => Some(*point),
        NativeDragAction::End {
            point: Some(point), ..
        } => Some(*point),
        _ => None,
    };
    if mouse.is_some_and(|mouse| mouse_point.is_some_and(|point| point != mouse.client_point)) {
        return Err("Native drag mouse point does not match its action".into());
    }
    let point = match &action {
        NativeDragAction::Enter { point, .. }
        | NativeDragAction::Drop { point, .. }
        | NativeDragAction::Start { point, .. } => Some(*point),
        _ => None,
    };
    if point.is_some_and(|point| {
        tabs.get(tab).is_none_or(|page| {
            frame != page.frame_generation() || !page.validate_menu_point(point.x, point.y)
        })
    }) {
        return Err("Native drag frame is stale or outside the viewport".into());
    }
    let source_point = match &action {
        NativeDragAction::SourceMove { point, .. } => Some((*point, true)),
        NativeDragAction::End {
            point: Some(point), ..
        } => Some((*point, false)),
        _ => None,
    };
    if let Some((point, check_frame)) = source_point {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x.abs() > 16384.0
            || point.y.abs() > 16384.0
            || check_frame
                && tabs
                    .get(tab)
                    .is_none_or(|page| frame != page.frame_generation())
        {
            return Err(
                "Native drag source motion is stale or outside its coordinate budget".into(),
            );
        }
    }
    match action {
        NativeDragAction::Enter {
            drag_id,
            point,
            types,
            allowed_operations,
        } => {
            update_destination(
                tabs,
                tab,
                source,
                context,
                drag_id,
                point,
                types,
                allowed_operations,
                mouse,
                executor,
                requests,
            )?;
            let page = tabs.get(tab).ok_or("Native drag tab has closed")?;
            let drag = page.current_native_drag(drag_id)?;
            Ok(page.native_drag_reply(source, drag_id, true, drag.operation, None, mouse))
        }
        NativeDragAction::Leave { drag_id } => {
            let mut drag = tabs
                .get_mut(tab)
                .ok_or("Native drag tab has closed")?
                .end_native_drag_destination(drag_id)?;
            if mouse.is_some() {
                drag.mouse = mouse;
            }
            if let Some(target) = drag.target {
                fire_event(
                    tabs,
                    tab,
                    source,
                    context,
                    target,
                    event_for(&drag, Kind::DragLeave),
                    executor,
                    requests,
                )?;
            }
            flush_tasks(tabs, tab, executor, requests)?;
            validate_document(tabs, tab, source, context)?;
            let page = tabs.get(tab).ok_or("Native drag tab has closed")?;
            Ok(page.native_drag_reply(source, drag_id, false, Operation::None, None, mouse))
        }
        NativeDragAction::Drop {
            drag_id,
            point,
            items,
            expected_operation,
        } => {
            let original = tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .validate_native_drag_items(drag_id, &items)?;
            // A drop is hit-tested again. It cannot reuse a cached permitted
            // target after the pointer or DOM geometry has changed.
            update_destination(
                tabs,
                tab,
                source,
                context,
                drag_id,
                point,
                original.types,
                original.allowed,
                mouse,
                executor,
                requests,
            )?;
            let drag = tabs
                .get_mut(tab)
                .ok_or("Native drag tab has closed")?
                .end_native_drag_destination(drag_id)?;
            let mut operation = drag.operation;
            if expected_operation.is_some_and(|expected| expected != operation) {
                operation = Operation::None;
            }
            let mut changed = None;
            if operation != Operation::None {
                if let Some(target) = drag.target {
                    let mut event = event_for(&drag, Kind::Drop);
                    event.items = items.clone();
                    let outcome = fire_event(
                        tabs, tab, source, context, target, event, executor, requests,
                    )?;
                    if let Some(outcome) = &outcome {
                        operation = if drag.allowed.contains(&outcome.operation)
                            && expected_operation
                                .is_none_or(|expected| expected == outcome.operation)
                        {
                            outcome.operation
                        } else {
                            Operation::None
                        };
                    }
                    if operation != Operation::None
                        && !outcome.is_some_and(|outcome| outcome.default_prevented)
                    {
                        let page = tabs.get_mut(tab).ok_or("Native drag tab has closed")?;
                        let mut drop = drag.clone();
                        drop.operation = operation;
                        if page.native_drag_target(point) == drop.target
                            && page.native_drag_default_operation(&drop, source) != Operation::None
                        {
                            let (accepted, edited) =
                                page.apply_native_drop_default(&drop, source, items)?;
                            changed = edited;
                            if !accepted {
                                operation = Operation::None;
                            }
                        } else {
                            operation = Operation::None;
                        }
                    }
                }
            }
            if let Some(node) = changed {
                dispatch_input_events(tabs, tab, source, context, node, executor, requests)?;
            }
            flush_tasks(tabs, tab, executor, requests)?;
            validate_document(tabs, tab, source, context)?;
            let page = tabs.get(tab).ok_or("Native drag tab has closed")?;
            Ok(page.native_drag_reply(source, drag_id, false, operation, None, mouse))
        }
        NativeDragAction::Start {
            drag_id,
            point,
            allowed_operations,
        } => {
            let mut drag = tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .native_drag_source_seed(drag_id, point, allowed_operations, source)?;
            if drag.allowed.is_empty() {
                return Err("Native drag source operation is unavailable".into());
            }
            tabs.get_mut(tab)
                .ok_or("Native drag tab has closed")?
                .reserve_native_drag_source(drag_id)?;
            drag.mouse = mouse;
            let mut event = PageHostDragEvent::new(Kind::DragStart, point);
            event.set_mouse(mouse);
            event.items = drag.items.clone();
            event.types = formats(&drag.items);
            event.effect_allowed = effect_allowed(&drag.allowed).into();
            let outcome = fire_event(
                tabs,
                tab,
                source,
                context,
                drag.target,
                event,
                executor,
                requests,
            )?;
            if outcome
                .as_ref()
                .is_some_and(|outcome| outcome.default_prevented)
            {
                flush_tasks(tabs, tab, executor, requests)?;
                validate_document(tabs, tab, source, context)?;
                return Ok(tabs
                    .get(tab)
                    .ok_or("Native drag tab has closed")?
                    .native_drag_reply(source, drag_id, false, Operation::None, None, mouse));
            }
            let items = if let Some(outcome) = outcome {
                drag.allowed = restrict_operations(&drag.allowed, &outcome.effect_allowed);
                outcome.items
            } else {
                drag.items.clone()
            };
            let operation = drag.allowed.first().copied().unwrap_or(Operation::None);
            let page = tabs.get_mut(tab).ok_or("Native drag tab has closed")?;
            page.install_native_drag_source(drag, items.clone())?;
            flush_tasks(tabs, tab, executor, requests)?;
            validate_document(tabs, tab, source, context)?;
            Ok(tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .native_drag_reply(source, drag_id, true, operation, Some(items), mouse))
        }
        NativeDragAction::SourceMove { drag_id, point } => {
            let drag = tabs
                .get_mut(tab)
                .ok_or("Native drag tab has closed")?
                .move_native_drag_source(drag_id, point, mouse)?;
            let mut event = PageHostDragEvent::new(Kind::Drag, point);
            event.set_mouse(drag.mouse);
            event.types = formats(&drag.items);
            event.effect_allowed = effect_allowed(&drag.allowed).into();
            let cancelled = fire_event(
                tabs,
                tab,
                source,
                context,
                drag.target,
                event.clone(),
                executor,
                requests,
            )?
            .is_some_and(|outcome| outcome.default_prevented);
            if cancelled {
                tabs.get_mut(tab)
                    .ok_or("Native drag tab has closed")?
                    .end_native_drag_source(drag_id, source, Operation::None)?;
                event.kind = Kind::DragEnd;
                fire_event(
                    tabs,
                    tab,
                    source,
                    context,
                    drag.target,
                    event,
                    executor,
                    requests,
                )?;
            }
            flush_tasks(tabs, tab, executor, requests)?;
            validate_document(tabs, tab, source, context)?;
            Ok(tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .native_drag_reply(source, drag_id, !cancelled, Operation::None, None, mouse))
        }
        NativeDragAction::End {
            drag_id,
            operation,
            point,
        } => {
            if let Some(point) = point.or(mouse.map(|mouse| mouse.client_point)) {
                tabs.get_mut(tab)
                    .ok_or("Native drag tab has closed")?
                    .move_native_drag_source(drag_id, point, mouse)?;
            }
            let drag = tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .current_native_drag_source(drag_id)?;
            let changed = tabs
                .get_mut(tab)
                .ok_or("Native drag tab has closed")?
                .end_native_drag_source(drag_id, source, operation)?;
            if let Some(node) = changed {
                dispatch_input_events(tabs, tab, source, context, node, executor, requests)?;
            }
            let mut event = PageHostDragEvent::new(Kind::DragEnd, drag.point);
            event.set_mouse(drag.mouse);
            event.types = formats(&drag.items);
            event.operation = operation;
            event.effect_allowed = effect_allowed(&drag.allowed).into();
            fire_event(
                tabs,
                tab,
                source,
                context,
                drag.target,
                event,
                executor,
                requests,
            )?;
            flush_tasks(tabs, tab, executor, requests)?;
            validate_document(tabs, tab, source, context)?;
            Ok(tabs
                .get(tab)
                .ok_or("Native drag tab has closed")?
                .native_drag_reply(source, drag_id, false, operation, None, mouse))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_destination(
    tabs: &mut TabManager,
    tab: TabId,
    source: u64,
    context: &PointerContext,
    id: u64,
    point: PointerPoint,
    types: Vec<String>,
    allowed: Vec<Operation>,
    mouse: Option<NativeDragMouse>,
    executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    requests: Option<&ScriptRequestReceiver>,
) -> Result<(), String> {
    let (previous, drag) = tabs
        .get_mut(tab)
        .ok_or("Native drag tab has closed")?
        .begin_native_drag(id, point, types, allowed, mouse)?;
    if previous
        .as_ref()
        .is_none_or(|previous| previous.id != drag.id || previous.target != drag.target)
    {
        if let Some(target) = drag.target {
            fire_event(
                tabs,
                tab,
                source,
                context,
                target,
                event_for(&drag, Kind::DragEnter),
                executor,
                requests,
            )?;
        }
        if let Some(mut previous) = previous {
            if let Some(target) = previous.target {
                previous.point = point;
                previous.mouse = mouse;
                let mut event = event_for(&previous, Kind::DragLeave);
                // A replacement OS session does not inherit the related
                // node from the preceding transaction.
                if previous.id == drag.id {
                    event.related_target = drag.target.map(|node| node.as_u64());
                }
                fire_event(
                    tabs, tab, source, context, target, event, executor, requests,
                )?;
            }
        }
    }
    let operation = if let Some(target) = drag.target {
        let outcome = fire_event(
            tabs,
            tab,
            source,
            context,
            target,
            event_for(&drag, Kind::DragOver),
            executor,
            requests,
        )?;
        if let Some(outcome) = outcome.filter(|outcome| outcome.default_prevented) {
            if drag.allowed.contains(&outcome.operation) {
                outcome.operation
            } else {
                Operation::None
            }
        } else {
            let page = tabs.get(tab).ok_or("Native drag tab has closed")?;
            if page.native_drag_target(point) == drag.target {
                page.native_drag_default_operation(&drag, source)
            } else {
                Operation::None
            }
        }
    } else {
        Operation::None
    };
    tabs.get_mut(tab)
        .ok_or("Native drag tab has closed")?
        .set_native_drag_operation(id, operation)
}

#[allow(clippy::too_many_arguments)]
fn fire_event(
    tabs: &mut TabManager,
    tab: TabId,
    source: u64,
    context: &PointerContext,
    node: NodeId,
    mut event: PageHostDragEvent,
    executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    requests: Option<&ScriptRequestReceiver>,
) -> Result<Option<PageHostDragOutcome>, String> {
    tabs.get(tab)
        .ok_or("Native drag tab has closed")?
        .set_native_drag_geometry(node, &mut event);
    let result = match executor.as_deref_mut() {
        Some(executor) => executor
            .dispatch_drag_serving_script(tabs, tab, node.as_u64(), event, requests)
            .map_err(|_| "Page drag listener is unavailable".to_string())?,
        None => None,
    };
    validate_document(tabs, tab, source, context)?;
    Ok(result)
}

fn validate_document(
    tabs: &TabManager,
    tab: TabId,
    source: u64,
    context: &PointerContext,
) -> Result<(), String> {
    tabs.get(tab)
        .ok_or_else(|| "Native drag tab has closed".to_string())?
        .validate_native_hover_context(context, source)
}

fn event_for(drag: &DragDestination, kind: Kind) -> PageHostDragEvent {
    let mut event = PageHostDragEvent::new(kind, drag.point);
    event.set_mouse(drag.mouse);
    event.types = drag.types.clone();
    event.effect_allowed = effect_allowed(&drag.allowed).into();
    event.operation = if matches!(kind, Kind::DragEnter | Kind::DragOver) {
        [Operation::Copy, Operation::Move, Operation::Link]
            .into_iter()
            .find(|operation| drag.allowed.contains(operation))
            .unwrap_or(Operation::None)
    } else if matches!(kind, Kind::Drop | Kind::DragEnd) {
        drag.operation
    } else {
        Operation::None
    };
    event
}

fn formats(items: &[NativeDragItem]) -> Vec<String> {
    let mut formats = items
        .iter()
        .filter_map(|item| match item {
            NativeDragItem::Text { format, .. } => Some(format.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if items
        .iter()
        .any(|item| matches!(item, NativeDragItem::File(_)))
    {
        formats.push("Files".into());
    }
    formats
}

fn dispatch_input_events(
    tabs: &mut TabManager,
    tab: TabId,
    source: u64,
    context: &PointerContext,
    node: NodeId,
    executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    requests: Option<&ScriptRequestReceiver>,
) -> Result<(), String> {
    let file = tabs.get(tab).is_some_and(|page| {
        page.file_input_state(node.as_u64(), source, context.document_generation)
            .is_ok()
    });
    let events = if file {
        vec![
            blueice_ipc::page_host::PageHostFileSelectionEvent::Input,
            blueice_ipc::page_host::PageHostFileSelectionEvent::Change,
        ]
    } else {
        vec![blueice_ipc::page_host::PageHostFileSelectionEvent::Input]
    };
    if let Some(executor) = executor.as_deref_mut() {
        for event in events {
            executor
                .dispatch_file_selection_serving_script(tabs, tab, node.as_u64(), event, requests)
                .map_err(|_| "Page drop input listener is unavailable".to_string())?;
            validate_document(tabs, tab, source, context)?;
        }
    }
    Ok(())
}

fn flush_tasks(
    tabs: &mut TabManager,
    tab: TabId,
    executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    requests: Option<&ScriptRequestReceiver>,
) -> Result<(), String> {
    if let Some(executor) = executor.as_deref_mut() {
        executor
            .flush_drag_tasks_serving_script(tabs, tab, requests)
            .map_err(|_| "Page drag task is unavailable".to_string())?;
    }
    Ok(())
}
