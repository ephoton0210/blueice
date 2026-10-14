// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) enum ChildExecutionReport {
    JavaScript(JavaScriptPageExecutionReport),
    BlueTs(BlueTsPageExecutionReport),
}

pub(super) fn child_report(
    report: page_host::PageHostScriptReport,
    source_position: Option<InlineBlueTsSourcePosition>,
) -> ChildExecutionReport {
    match report.language {
        PageHostScriptLanguage::JavaScript => match report.outcome {
            PageHostScriptOutcome::Executed => {
                ChildExecutionReport::JavaScript(JavaScriptPageExecutionReport::Executed {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_js_kind(report.kind),
                })
            }
            PageHostScriptOutcome::Rejected { category } => {
                ChildExecutionReport::JavaScript(JavaScriptPageExecutionReport::Rejected {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_js_kind(report.kind),
                    category: leak_category(category),
                })
            }
        },
        PageHostScriptLanguage::BlueTs => match report.outcome {
            PageHostScriptOutcome::Executed => {
                ChildExecutionReport::BlueTs(BlueTsPageExecutionReport::Executed {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_blue_ts_kind(report.kind),
                })
            }
            PageHostScriptOutcome::Rejected { category } => {
                ChildExecutionReport::BlueTs(BlueTsPageExecutionReport::Rejected {
                    tab_id: report.tab_id,
                    document_generation: report.document_generation,
                    ordinal: report.ordinal,
                    kind: core_blue_ts_kind(report.kind),
                    category: leak_blue_ts_category(category),
                    source_position,
                })
            }
        },
    }
}

pub(super) fn verified_child_inline_position(
    report: &page_host::PageHostScriptReport,
    tab_id: TabId,
    document_generation: u64,
    inline_sources: &BTreeMap<u32, String>,
) -> Option<InlineBlueTsSourcePosition> {
    if report.tab_id != tab_id.as_u64()
        || report.document_generation != document_generation
        || report.language != PageHostScriptLanguage::BlueTs
    {
        return None;
    }
    let PageHostScriptOutcome::Rejected { category } = &report.outcome else {
        return None;
    };
    if !matches!(
        category.as_str(),
        "BlueTS compilation rejected the page script"
            | "BlueTS direct lowering rejected the page script"
    ) {
        return None;
    }
    let candidate = report.source_position.as_ref()?;
    if !candidate.is_well_formed() {
        return None;
    }
    let source = inline_sources.get(&report.ordinal)?;
    let expected_module = inline_module_id(
        tab_id,
        document_generation,
        report.ordinal,
        CombinedPageScriptLanguage::BlueTs(core_blue_ts_kind(report.kind)),
    );
    let start = candidate.start as usize;
    let end = candidate.end as usize;
    if candidate.module_id != expected_module
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    Some(InlineBlueTsSourcePosition {
        start: candidate.start,
        end: candidate.end,
    })
}

// `JavaScriptPageExecutionReport` predates the child transport and stores
// fixed categories as `&'static str`. The child receives only categories this
// core itself recognizes; normalize unknown/new labels to one fixed value
// rather than retaining peer-owned allocation beyond a request.
pub(super) fn leak_category(category: String) -> &'static str {
    match category.as_str() {
        "JavaScript parsing rejected the page script" => {
            "JavaScript parsing rejected the page script"
        }
        "BlueJS compilation rejected the page script" => {
            "BlueJS compilation rejected the page script"
        }
        "JavaScript page resource policy rejected the page script" => {
            "JavaScript page resource policy rejected the page script"
        }
        "authorized JavaScript graph rejected the page script" => {
            "authorized JavaScript graph rejected the page script"
        }
        "BlueJS page execution failed" => "BlueJS page execution failed",
        "BlueJS page host rejected the page script" => "BlueJS page host rejected the page script",
        "authorized JavaScript source record is invalid" => {
            "authorized JavaScript source record is invalid"
        }
        "authorized JavaScript graph is missing a static resolution" => {
            "authorized JavaScript graph is missing a static resolution"
        }
        _ => "out-of-process JavaScript host rejected the page script",
    }
}

pub(super) fn leak_blue_ts_category(category: String) -> &'static str {
    match category.as_str() {
        "BlueTS compilation rejected the page script" => {
            "BlueTS compilation rejected the page script"
        }
        "BlueTS direct lowering rejected the page script" => {
            "BlueTS direct lowering rejected the page script"
        }
        "BlueJS compilation rejected the direct BlueTS page script" => {
            "BlueJS compilation rejected the direct BlueTS page script"
        }
        "JavaScript page resource policy rejected the page script" => {
            "JavaScript page resource policy rejected the page script"
        }
        "authorized JavaScript graph rejected the page script" => {
            "authorized JavaScript graph rejected the page script"
        }
        "BlueJS page execution failed" => "BlueJS page execution failed",
        "BlueJS page host rejected the page script" => "BlueJS page host rejected the page script",
        "authorized BlueTS graph is invalid" => "authorized BlueTS graph is invalid",
        _ => "out-of-process BlueTS host rejected the page script",
    }
}

pub(super) fn child_error_category(code: PageHostErrorCode) -> &'static str {
    match code {
        PageHostErrorCode::ResourceLimit => {
            "out-of-process JavaScript host resource policy rejected the document"
        }
        PageHostErrorCode::StaleDocument => {
            "out-of-process JavaScript host rejected a stale document"
        }
        PageHostErrorCode::Authentication
        | PageHostErrorCode::ProtocolVersion
        | PageHostErrorCode::InvalidRequest
        | PageHostErrorCode::InvalidDebuggerState
        | PageHostErrorCode::UnknownRealm
        | PageHostErrorCode::HostFailure => "out-of-process JavaScript host rejected the document",
    }
}

pub(super) fn rejected_report(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    kind: BlueJsPageScriptKind,
    category: &'static str,
) -> JavaScriptPageExecutionReport {
    JavaScriptPageExecutionReport::Rejected {
        tab_id: tab_id.as_u64(),
        document_generation,
        ordinal,
        kind,
        category,
    }
}

pub(super) fn rejected_blue_ts_report(
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    kind: DirectPageScriptKind,
    category: &'static str,
) -> BlueTsPageExecutionReport {
    BlueTsPageExecutionReport::Rejected {
        tab_id: tab_id.as_u64(),
        document_generation,
        ordinal,
        kind,
        category,
        source_position: None,
    }
}

pub(super) fn push_child_failure_report(
    (java_script_reports, blue_ts_reports): (
        &mut Vec<JavaScriptPageExecutionReport>,
        &mut Vec<BlueTsPageExecutionReport>,
    ),
    tab_id: TabId,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    category: &'static str,
) {
    match language {
        PageHostScriptLanguage::JavaScript => java_script_reports.push(rejected_report(
            tab_id,
            document_generation,
            ordinal,
            core_js_kind(kind),
            category,
        )),
        PageHostScriptLanguage::BlueTs => blue_ts_reports.push(rejected_blue_ts_report(
            tab_id,
            document_generation,
            ordinal,
            core_blue_ts_kind(kind),
            category,
        )),
    }
}

pub(super) fn child_language(language: CombinedPageScriptLanguage) -> PageHostScriptLanguage {
    match language {
        CombinedPageScriptLanguage::JavaScript(_) => PageHostScriptLanguage::JavaScript,
        CombinedPageScriptLanguage::BlueTs(_) => PageHostScriptLanguage::BlueTs,
    }
}

pub(super) fn child_kind(language: CombinedPageScriptLanguage) -> PageHostScriptKind {
    match language {
        CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic)
        | CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic) => {
            PageHostScriptKind::Classic
        }
        CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module)
        | CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module) => {
            PageHostScriptKind::Module
        }
    }
}

pub(super) fn core_js_kind(kind: PageHostScriptKind) -> BlueJsPageScriptKind {
    match kind {
        PageHostScriptKind::Classic => BlueJsPageScriptKind::Classic,
        PageHostScriptKind::Module => BlueJsPageScriptKind::Module,
    }
}

pub(super) fn core_blue_ts_kind(kind: PageHostScriptKind) -> DirectPageScriptKind {
    match kind {
        PageHostScriptKind::Classic => DirectPageScriptKind::Classic,
        PageHostScriptKind::Module => DirectPageScriptKind::Module,
    }
}

pub(super) fn report_tab_id(report: &JavaScriptPageExecutionReport) -> u64 {
    match report {
        JavaScriptPageExecutionReport::Executed { tab_id, .. }
        | JavaScriptPageExecutionReport::Rejected { tab_id, .. } => *tab_id,
    }
}

pub(super) fn report_ordinal(report: &JavaScriptPageExecutionReport) -> u32 {
    match report {
        JavaScriptPageExecutionReport::Executed { ordinal, .. }
        | JavaScriptPageExecutionReport::Rejected { ordinal, .. } => *ordinal,
    }
}

pub(super) fn blue_ts_report_tab_id(report: &BlueTsPageExecutionReport) -> u64 {
    match report {
        BlueTsPageExecutionReport::Executed { tab_id, .. }
        | BlueTsPageExecutionReport::Rejected { tab_id, .. } => *tab_id,
    }
}

pub(super) fn blue_ts_report_ordinal(report: &BlueTsPageExecutionReport) -> u32 {
    match report {
        BlueTsPageExecutionReport::Executed { ordinal, .. }
        | BlueTsPageExecutionReport::Rejected { ordinal, .. } => *ordinal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_position_requires_exact_inline_identity_generation_and_utf8_range() {
        let tab_id = TabId::from_u64(7);
        let source = "const café: number = \"wrong\";";
        let inline_sources = BTreeMap::from([(3, source.to_string())]);
        let mut report = page_host::PageHostScriptReport {
            tab_id: 7,
            document_generation: 4,
            ordinal: 3,
            language: PageHostScriptLanguage::BlueTs,
            kind: PageHostScriptKind::Classic,
            source_position: Some(page_host::PageHostScriptSourcePosition {
                module_id: inline_module_id(
                    tab_id,
                    4,
                    3,
                    CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic),
                ),
                start: 6,
                end: 11,
            }),
            outcome: PageHostScriptOutcome::Rejected {
                category: "BlueTS compilation rejected the page script".into(),
            },
        };
        let verify = |report: &page_host::PageHostScriptReport| {
            verified_child_inline_position(report, tab_id, 4, &inline_sources)
        };
        assert_eq!(
            verify(&report),
            Some(InlineBlueTsSourcePosition { start: 6, end: 11 })
        );
        report.source_position.as_mut().unwrap().end = 10;
        assert_eq!(verify(&report), None, "partial UTF-8 code point");
        report.source_position.as_mut().unwrap().end = (source.len() + 1) as u32;
        assert_eq!(verify(&report), None, "outside original inline source");
        report.source_position.as_mut().unwrap().end = 11;
        report.source_position.as_mut().unwrap().module_id = inline_module_id(
            tab_id,
            3,
            3,
            CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic),
        );
        assert_eq!(verify(&report), None, "stale module identity");
        report.source_position.as_mut().unwrap().module_id = "private-external.ts".into();
        assert_eq!(verify(&report), None, "external module identity");
        report.source_position.as_mut().unwrap().module_id = inline_module_id(
            tab_id,
            4,
            3,
            CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic),
        );
        report.tab_id = 8;
        assert_eq!(verify(&report), None, "wrong tab");
        report.tab_id = 7;
        report.document_generation = 5;
        assert_eq!(verify(&report), None, "wrong document generation");
        report.document_generation = 4;
        report.outcome = PageHostScriptOutcome::Rejected {
            category: "BlueJS page execution failed".into(),
        };
        assert_eq!(
            verify(&report),
            None,
            "runtime failure has no compiler span"
        );
        report.outcome = PageHostScriptOutcome::Rejected {
            category: "BlueTS compilation rejected the page script".into(),
        };
        report.ordinal = 2;
        assert_eq!(verify(&report), None, "unknown inline declaration");
    }
}
