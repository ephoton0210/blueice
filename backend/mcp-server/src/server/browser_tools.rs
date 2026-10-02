// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[tool_router(router = browser_tools_router, vis = "pub(super)")]
impl BlueIceMcpServer {
    #[tool(
        description = "Navigate to a URL and return the resulting page representation (an accessibility-tree-shaped snapshot, per phase-1-ai-representation-layer/PLAN.md)"
    )]
    pub(super) async fn navigate(
        &self,
        Parameters(NavigateParams { url, tab_id }): Parameters<NavigateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| conn.navigate(&url, tab_id)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Restore the previous session-history entry for a tab and return its restored page representation. Omit tab_id only for the default tab."
    )]
    pub(super) async fn go_back(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| conn.go_back(tab_id)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Restore the next session-history entry for a tab and return its restored page representation. Omit tab_id only for the default tab."
    )]
    pub(super) async fn go_forward(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| conn.go_forward(tab_id)).await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Choose the language that pages fetched from now on are translated into by BlueIce's local assistant (a BCP 47 tag such as zh-TW), or omit target_language to turn translation off. Already-loaded pages change only through show_translation or a reload. Translated nodes keep the page's own words in original_name. Fails when BlueIce was started without an assistant."
    )]
    pub(super) async fn set_translation_language(
        &self,
        Parameters(SetTranslationLanguageParams {
            target_language,
            tab_id,
        }): Parameters<SetTranslationLanguageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.set_translation_language(target_language, tab_id)
        })
        .await?;
        Ok(translation_outcome_to_result(outcome))
    }

    #[tool(
        description = "Show a tab's translated text (shown: true) or the page's original text (shown: false) and return the resulting page representation. Only a page that was translated when it loaded can be toggled; check translation.available."
    )]
    pub(super) async fn show_translation(
        &self,
        Parameters(ShowTranslationParams { shown, tab_id }): Parameters<ShowTranslationParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.show_translation(shown, tab_id)
        })
        .await?;
        Ok(translation_outcome_to_result(outcome))
    }

    #[tool(
        description = "Ask BlueIce's local assistant to summarize a tab's shown text. The result is model output derived from untrusted page text, is also added to the about:assistant page, and is clearly delimited. Fails when BlueIce was started without an assistant or the page has no text."
    )]
    pub(super) async fn summarize_page(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| conn.summarize_page(tab_id)).await?;
        Ok(assistant_outcome_to_result(outcome))
    }

    #[tool(
        description = "Ask BlueIce's local assistant to reorganize a tab's shown text per an instruction (for example a table of names and prices). The result is model output derived from untrusted page text, is also added to the about:assistant page, and is clearly delimited."
    )]
    pub(super) async fn organize_page(
        &self,
        Parameters(OrganizePageParams {
            instruction,
            tab_id,
        }): Parameters<OrganizePageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.organize_page(instruction, tab_id)
        })
        .await?;
        Ok(assistant_outcome_to_result(outcome))
    }

    #[tool(
        description = "Debugging aid: what the BlueIce launcher is doing right now -- its pid, the core's pid and generation (bumped by every hot-swap cutover), whether the local assistant is running and how often it was started, and any settings proposal waiting for the person. Read-only; carries no settings values or page data."
    )]
    pub(super) async fn blueice_status(&self) -> Result<CallToolResult, ErrorData> {
        let socket = self.control_socket.clone();
        let text = control_call(move || assistant_settings::launcher_status(&socket)).await?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Read the local assistant's settings in force (backend, model, memory ceiling, priority). Read-only."
    )]
    pub(super) async fn get_assistant_settings(&self) -> Result<CallToolResult, ErrorData> {
        let socket = self.control_socket.clone();
        let outcome = control_call(move || assistant_settings::current(&socket)).await?;
        Ok(assistant_settings_result(outcome))
    }

    #[tool(
        description = "PROPOSE a change to the local assistant's settings. You cannot apply it: deterministic safety rules screen it first (a blocked proposal is returned with the reasons and the person is not asked), and an accepted one only takes effect if the person approves it in BlueIce's own trusted window, which you cannot reach. Nothing changes until then. At most one proposal can wait at a time."
    )]
    pub(super) async fn propose_assistant_settings(
        &self,
        Parameters(ProposeAssistantSettingsParams { settings }): Parameters<
            ProposeAssistantSettingsParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        let settings = match settings.into_settings() {
            Ok(settings) => settings,
            Err(reason) => return Ok(CallToolResult::error(vec![Content::text(reason)])),
        };
        let socket = self.control_socket.clone();
        let outcome = control_call(move || assistant_settings::propose(&socket, settings)).await?;
        Ok(assistant_settings_result(outcome))
    }

    #[tool(
        description = "Check where a settings proposal stands: pending, approved, denied, expired, stale (the settings changed after it was made), or unknown. Read-only."
    )]
    pub(super) async fn assistant_settings_proposal_status(
        &self,
        Parameters(ProposalStatusParams { id }): Parameters<ProposalStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let socket = self.control_socket.clone();
        let outcome = control_call(move || assistant_settings::status(&socket, id)).await?;
        Ok(assistant_settings_result(outcome))
    }

    #[tool(description = "Get the current page's representation without performing any action")]
    pub(super) async fn get_page_representation(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let snapshot = blocking(self.core.clone(), move |conn| conn.representation(tab_id)).await?;
        let text = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Ok(CallToolResult::success(vec![Content::text(
            crate::wrap_untrusted_page_content(&text),
        )]))
    }

    #[tool(
        description = "Get the full DOM tree as a canonical text dump, unfiltered by the AI representation's semantic-role/display:none exclusion -- useful for structural comparison against another browser's DOM"
    )]
    pub(super) async fn get_dom(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dump = blocking(self.core.clone(), move |conn| conn.dom(tab_id)).await?;
        Ok(CallToolResult::success(vec![Content::text(
            crate::wrap_untrusted_page_content(&dump),
        )]))
    }

    #[tool(
        description = "Click the element with this node ID (follows a link's href if it is or is inside one, same as a human click)"
    )]
    pub(super) async fn click(
        &self,
        Parameters(NodeIdParams { node_id, tab_id }): Parameters<NodeIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.act(node_id, NodeAction::Click, tab_id)
        })
        .await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Set the value of an input/textarea/select element identified by node ID")]
    pub(super) async fn type_text(
        &self,
        Parameters(TypeTextParams {
            node_id,
            text,
            tab_id,
        }): Parameters<TypeTextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.act(node_id, NodeAction::SetValue(text), tab_id)
        })
        .await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(description = "Move keyboard focus to the element with this node ID")]
    pub(super) async fn focus(
        &self,
        Parameters(NodeIdParams { node_id, tab_id }): Parameters<NodeIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.act(node_id, NodeAction::Focus, tab_id)
        })
        .await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Scroll the page so the element with this node ID is aligned to the top of the viewport"
    )]
    pub(super) async fn scroll_into_view(
        &self,
        Parameters(NodeIdParams { node_id, tab_id }): Parameters<NodeIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.act(node_id, NodeAction::ScrollIntoView, tab_id)
        })
        .await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Highlight an element for the human-visible window (an outline drawn around its current bounds), or clear the highlight by omitting node_id"
    )]
    pub(super) async fn highlight(
        &self,
        Parameters(HighlightParams { node_id, tab_id }): Parameters<HighlightParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.highlight(node_id, tab_id)
        })
        .await?;
        Ok(outcome_to_result(outcome))
    }

    #[tool(
        description = "Take a PNG screenshot of the most recently rendered frame for a tab (call navigate/open_tab on it first; there is nothing to screenshot before that). The text result identifies the frame source, tab_id and generation encoded in the PNG; all three are needed across core cutovers. Omit tab_id for the tab most recently rendered by this MCP connection's own request; an unsolicited human-tab refresh never changes that default."
    )]
    pub(super) async fn screenshot(
        &self,
        Parameters(GetPageParams { tab_id }): Parameters<GetPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let screenshot = blocking(self.core.clone(), move |conn| {
            let Some((resolved_tab_id, frame)) = conn.last_frame_with_tab_id(tab_id) else {
                return Ok(None);
            };
            let mapped = blueice_ipc::shm::map_frame(std::path::Path::new(&frame.shm_path))?;
            let png = crate::frame_to_png_bytes(&mapped, frame.width, frame.height)?;
            Ok(Some((
                png,
                resolved_tab_id,
                frame.generation,
                frame.frame_source(),
            )))
        })
        .await?;

        match screenshot {
            Some((bytes, resolved_tab_id, generation, frame_source)) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                // A rendered page can bake adversarial text directly
                // into its pixels (visual prompt injection against a
                // vision-capable reader), same threat class as
                // `wrap_untrusted_page_content` defends against for
                // text tool results -- so this image gets the same
                // warning as a leading text block, not just the text
                // tools.
                let warning = crate::wrap_untrusted_page_content("(see attached image)");
                let metadata = format!(
                    "{}{{\"frame_source\":{frame_source},\"tab_id\":{resolved_tab_id},\"generation\":{generation}}}",
                    crate::FRAME_EVIDENCE_PREFIX
                );
                Ok(CallToolResult::success(vec![
                    Content::text(format!("{metadata}\n{warning}")),
                    Content::image(b64, "image/png"),
                ]))
            }
            None => Ok(CallToolResult::error(vec![Content::text(
                "no frame has been rendered yet for that tab -- call navigate/open_tab first",
            )])),
        }
    }

    #[tool(
        description = "List every currently open tab (id and url). Use the returned tab_id with navigate/click/get_page_representation/etc. to address a specific tab -- there is no single 'current tab' tracked by core itself, since a human and an AI may be looking at different tabs at once."
    )]
    pub(super) async fn list_tabs(&self) -> Result<CallToolResult, ErrorData> {
        let tabs = blocking(self.core.clone(), |conn| conn.list_tabs()).await?;
        let text = serde_json::to_string_pretty(&tabs).unwrap_or_else(|_| "[]".to_string());
        Ok(CallToolResult::success(vec![Content::text(
            crate::wrap_untrusted_page_content(&text),
        )]))
    }

    #[tool(
        description = "Open a new tab, optionally navigating it to a URL immediately. Returns the new tab's id -- pass it to other tools to address this tab specifically."
    )]
    pub(super) async fn open_tab(
        &self,
        Parameters(OpenTabParams { url }): Parameters<OpenTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome =
            blocking(self.core.clone(), move |conn| conn.open_tab(url.as_deref())).await?;
        match outcome {
            crate::OpenTabOutcome::Opened { tab_id, url } => {
                let text = serde_json::to_string_pretty(
                    &serde_json::json!({ "tab_id": tab_id, "url": url }),
                )
                .unwrap_or_else(|_| "{}".to_string());
                Ok(CallToolResult::success(vec![Content::text(
                    crate::wrap_untrusted_page_content(&text),
                )]))
            }
            crate::OpenTabOutcome::Error(message) => {
                Ok(CallToolResult::error(vec![Content::text(message)]))
            }
        }
    }

    #[tool(description = "Close a tab by id. Closing the last remaining tab is allowed.")]
    pub(super) async fn close_tab(
        &self,
        Parameters(CloseTabParams { tab_id }): Parameters<CloseTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| conn.close_tab(tab_id)).await?;
        match outcome {
            crate::CloseTabOutcome::Closed => Ok(CallToolResult::success(vec![Content::text(
                format!("tab {tab_id} closed"),
            )])),
            crate::CloseTabOutcome::Error(message) => {
                Ok(CallToolResult::error(vec![Content::text(message)]))
            }
        }
    }

    #[tool(
        description = "List core-owned tab groups (id, name, #RRGGBB color, collapsed state). Grouping is shared with the human tab strip; it is not an MCP-local current-tab setting."
    )]
    pub(super) async fn list_tab_groups(&self) -> Result<CallToolResult, ErrorData> {
        match blocking(self.core.clone(), |conn| conn.list_tab_groups()).await? {
            Ok(groups) => {
                let text =
                    serde_json::to_string_pretty(&groups).unwrap_or_else(|_| "[]".to_string());
                Ok(CallToolResult::success(vec![Content::text(
                    crate::wrap_untrusted_page_content(&text),
                )]))
            }
            Err(message) => Ok(CallToolResult::error(vec![Content::text(message)])),
        }
    }

    #[tool(
        description = "Create a named, colored tab group shared with the human frontend. color must be a CSS #RRGGBB value, for example #4f8cff."
    )]
    pub(super) async fn create_tab_group(
        &self,
        Parameters(CreateTabGroupParams { name, color }): Parameters<CreateTabGroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.create_tab_group(&name, &color)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }

    #[tool(
        description = "Add a tab to a shared tab group, or remove it from any group by omitting group_id. This never changes which tab another observer is viewing."
    )]
    pub(super) async fn set_tab_group(
        &self,
        Parameters(SetTabGroupParams { tab_id, group_id }): Parameters<SetTabGroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.set_tab_group(tab_id, group_id)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }

    #[tool(description = "Rename a shared tab group.")]
    pub(super) async fn rename_tab_group(
        &self,
        Parameters(RenameTabGroupParams { group_id, name }): Parameters<RenameTabGroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.rename_tab_group(group_id, &name)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }

    #[tool(description = "Set a shared tab group's CSS #RRGGBB color.")]
    pub(super) async fn set_tab_group_color(
        &self,
        Parameters(SetTabGroupColorParams { group_id, color }): Parameters<SetTabGroupColorParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.set_tab_group_color(group_id, &color)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }

    #[tool(
        description = "Collapse or expand a shared tab group in tab strips. Collapsing never closes or suspends its tabs."
    )]
    pub(super) async fn set_tab_group_collapsed(
        &self,
        Parameters(SetTabGroupCollapsedParams {
            group_id,
            collapsed,
        }): Parameters<SetTabGroupCollapsedParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.set_tab_group_collapsed(group_id, collapsed)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }

    #[tool(
        description = "Remove a shared tab group. Its member tabs stay open and become ungrouped."
    )]
    pub(super) async fn close_tab_group(
        &self,
        Parameters(TabGroupIdParams { group_id }): Parameters<TabGroupIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let outcome = blocking(self.core.clone(), move |conn| {
            conn.close_tab_group(group_id)
        })
        .await?;
        Ok(tab_group_outcome_to_result(outcome))
    }
}
