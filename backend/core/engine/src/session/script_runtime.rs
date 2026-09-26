// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn inline_execution_reports(
    reports: Vec<DirectPageScriptExecutionReport>,
) -> Vec<BlueTsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            DirectPageScriptExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_script_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Executed,
            },
            DirectPageScriptExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                message,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_script_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Rejected { category: message },
            },
        })
        .collect()
}

pub(super) fn inline_script_kind(kind: DirectPageScriptKind) -> BlueTsScriptKind {
    match kind {
        DirectPageScriptKind::Classic => BlueTsScriptKind::Classic,
        DirectPageScriptKind::Module => BlueTsScriptKind::Module,
    }
}

/// Converts core-owned standard JavaScript execution records into the public
/// source-free observation format. As with BlueTS reports, no source,
/// diagnostic, bytecode, program identity, or completion value can cross this
/// control-plane query.
pub(super) fn inline_javascript_execution_reports(
    reports: Vec<JavaScriptPageExecutionReport>,
) -> Vec<BlueJsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            JavaScriptPageExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueJsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_javascript_kind(kind),
                outcome: BlueJsScriptExecutionOutcome::Executed,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                category,
            } => BlueJsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: inline_javascript_kind(kind),
                outcome: BlueJsScriptExecutionOutcome::Rejected {
                    category: category.to_string(),
                },
            },
        })
        .collect()
}

pub(super) fn inline_javascript_kind(
    kind: crate::script::BlueJsPageScriptKind,
) -> BlueJsScriptKind {
    match kind {
        crate::script::BlueJsPageScriptKind::Classic => BlueJsScriptKind::Classic,
        crate::script::BlueJsPageScriptKind::Module => BlueJsScriptKind::Module,
    }
}

/// Converts the private child-host BlueTS outcomes into the existing
/// language-specific, source-free control-plane report shape. The direct
/// child remains the owner of compiler artifacts and runtime state.
pub(super) fn child_blue_ts_execution_reports(
    reports: Vec<BlueTsPageExecutionReport>,
) -> Vec<BlueTsScriptExecutionReport> {
    reports
        .into_iter()
        .map(|report| match report {
            BlueTsPageExecutionReport::Executed {
                tab_id,
                document_generation,
                ordinal,
                kind,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: child_blue_ts_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Executed,
            },
            BlueTsPageExecutionReport::Rejected {
                tab_id,
                document_generation,
                ordinal,
                kind,
                category,
            } => BlueTsScriptExecutionReport {
                tab_id,
                document_generation,
                ordinal,
                kind: child_blue_ts_kind(kind),
                outcome: BlueTsScriptExecutionOutcome::Rejected {
                    category: category.to_string(),
                },
            },
        })
        .collect()
}

pub(super) fn child_blue_ts_kind(
    kind: crate::script::direct_page::DirectPageScriptKind,
) -> BlueTsScriptKind {
    match kind {
        crate::script::direct_page::DirectPageScriptKind::Classic => BlueTsScriptKind::Classic,
        crate::script::direct_page::DirectPageScriptKind::Module => BlueTsScriptKind::Module,
    }
}

pub(super) fn synchronize_page_script_runtime(
    page_script_runtime: &mut PageScriptRuntime<'_>,
    tabs: &mut TabManager,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<()> {
    if let Some(direct_page_host) = page_script_runtime.direct_page_host.as_deref_mut() {
        direct_page_host.synchronize_tabs(tabs).map_err(|error| {
            io::Error::other(format!(
                "direct page lifecycle synchronization failed: {error}"
            ))
        })?;
    }
    synchronize_inline_page_executor(&mut page_script_runtime.inline_page_executor, tabs)?;
    synchronize_javascript_executor(
        &mut page_script_runtime.javascript_executor,
        tabs,
        script_requests,
    )?;
    Ok(())
}

pub(super) fn synchronize_javascript_executor(
    javascript_executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    tabs: &mut TabManager,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<()> {
    if let Some(javascript_executor) = javascript_executor.as_deref_mut() {
        javascript_executor.synchronize_and_execute_serving_script(tabs, script_requests)?;
    }
    Ok(())
}

pub(super) fn dispatch_click_before_default(
    javascript_executor: &mut Option<&mut dyn PageJavaScriptExecutor>,
    tabs: &mut TabManager,
    tab_id: TabId,
    node: NodeId,
    script_requests: Option<&ScriptRequestReceiver>,
) -> io::Result<Option<bool>> {
    match javascript_executor.as_deref_mut() {
        Some(executor) => {
            executor.dispatch_click_serving_script(tabs, tab_id, node.as_u64(), script_requests)
        }
        None => Ok(None),
    }
}

pub(super) fn synchronize_inline_page_executor(
    inline_page_executor: &mut Option<&mut DirectPageInlineExecutor>,
    tabs: &TabManager,
) -> io::Result<()> {
    if let Some(inline_page_executor) = inline_page_executor.as_deref_mut() {
        inline_page_executor
            .synchronize_and_execute(tabs)
            .map_err(|error| io::Error::other(format!("inline page execution failed: {error}")))?;
    }
    Ok(())
}
