// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[tool_router(router = locale_tools_router, vis = "pub(super)")]
impl BlueIceMcpServer {
    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's Intl.Collator service. Returns canonical locale input, every support decision, selected fallback, resolved options, and an optional exact UTF-16 comparison. It never executes JavaScript or reads/mutates browser state."
    )]
    pub(super) async fn debug_collator(
        &self,
        Parameters(params): Parameters<DebugCollatorParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_collator_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's decimal Intl.NumberFormat service. Returns canonical locale input, every support decision, selected fallback, resolved decimal options, and an optional formatted finite decimal. It never executes JavaScript or reads/mutates browser state. Currency, units, ranges, compact/scientific notation and non-finite symbols are not part of this decimal service slice."
    )]
    pub(super) async fn debug_number_format(
        &self,
        Parameters(params): Parameters<DebugNumberFormatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_number_format_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's Intl.PluralRules service. Returns canonical locale input, every support decision, selected fallback, resolved cardinal/ordinal options, and an optional category for an exact finite decimal string. It never executes JavaScript or reads/mutates browser state. Digit-option rounding and selectRange are not part of this initial service slice."
    )]
    pub(super) async fn debug_plural_rules(
        &self,
        Parameters(params): Parameters<DebugPluralRulesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_plural_rules_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's Intl.ListFormat service. Returns canonical locale input, every support decision, selected fallback, resolved type/style, input items, formatted list and element/literal formatToParts data. It never executes JavaScript or reads/mutates browser state."
    )]
    pub(super) async fn debug_list_format(
        &self,
        Parameters(params): Parameters<DebugListFormatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_list_format_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's Intl.Segmenter service. Returns canonical locale input, every support decision, selected fallback, resolved granularity and bounded segments with UTF-16 indices and word-likeness where applicable. It never executes JavaScript or reads/mutates browser state."
    )]
    pub(super) async fn debug_segmenter(
        &self,
        Parameters(params): Parameters<DebugSegmenterParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_segmenter_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Deep, read-only diagnostic for blueice-ecma402's Intl.Locale data. Returns the canonical tag, typed option application, optional likely-subtag transform, and calendars, collations, hour cycles, numbering systems, text direction, region time zones and week data. It never executes JavaScript or reads/mutates browser state."
    )]
    pub(super) async fn debug_locale(
        &self,
        Parameters(params): Parameters<DebugLocaleParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = debug_locale_report(params)
            .map_err(|message| ErrorData::invalid_params(message, None))?;
        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}
