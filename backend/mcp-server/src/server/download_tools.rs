// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[tool_router(router = download_tools_router, vis = "pub(super)")]
impl BlueIceMcpServer {
    #[tool(
        description = "Start downloading a file over HTTP(S), anonymous FTP as ftp://host/path, SFTP as sftp://user@host/path, or explicit FTPS as ftps://user@host/path, with BlueIce's built-in download manager. HTTP(S) and SFTP can use several connections at once; only HTTP(S) retains a partial file after a pause when the server supplies a validator. FTP-family transfers are single-stream and restart from the beginning after a pause. SFTP verifies the host against known-hosts and can use an SSH agent, a configured private key, or a saved password; FTPS verifies the TLS certificate and can use a saved password. Passwords in URLs are refused, and credential-setting is intentionally a local stdin-only CLI operation rather than an MCP tool. \
        Returns as soon as the transfer is queued -- it does NOT wait for the download to finish; read progress with get_transfer or list_transfers. \
        Every download passes through the local gatekeeper hook and can end up 'blocked' instead of downloading (the result says why). SECURITY LIMITATION: the current gatekeeper is an always-clear stub, and private or link-local network URLs are not blocked in this phase; do not treat this as malware scanning, authorization, or SSRF protection. \
        `dest` is an optional path relative to the download directory (absolute paths and '..' are refused); without it the name comes from the server or the URL. \
        An existing file is never replaced unless `overwrite` is true."
    )]
    pub(super) async fn download_file(
        &self,
        Parameters(DownloadFileParams {
            url,
            dest,
            overwrite,
        }): Parameters<DownloadFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let overwrite = overwrite.unwrap_or(false);
        // Not idempotent: a `start` whose reply was lost must not be run again.
        let outcome = downloads_call(self.downloads.clone(), false, move |c| {
            c.start(&url, dest.as_deref(), overwrite)
        })
        .await;
        Ok(transfer_result(outcome))
    }

    #[tool(
        description = "Remove the saved SFTP password for a host, port, and username from this machine's operating-system credential store. This does not alter any downloaded files or transfer history."
    )]
    pub(super) async fn remove_sftp_password(
        &self,
        Parameters(RemoveSftpPasswordParams {
            host,
            port,
            username,
        }): Parameters<RemoveSftpPasswordParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let port = port.unwrap_or(22);
        match downloads_call(self.downloads.clone(), true, move |client| {
            client.remove_sftp_password(&host, port, &username)
        })
        .await
        {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Saved SFTP password removed from the local operating-system credential store.",
            )])),
            Err(error) => Ok(call_error_result(error)),
        }
    }

    #[tool(
        description = "Remove the saved passphrase for the SFTP private key configured for a host, port, and username. This does not alter the key file, downloaded files, or transfer history."
    )]
    pub(super) async fn remove_sftp_private_key_passphrase(
        &self,
        Parameters(RemoveSftpPrivateKeyPassphraseParams {
            host,
            port,
            username,
        }): Parameters<RemoveSftpPrivateKeyPassphraseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let port = port.unwrap_or(22);
        match downloads_call(self.downloads.clone(), true, move |client| {
            client.remove_sftp_private_key_passphrase(&host, port, &username)
        })
        .await
        {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Saved SFTP private-key passphrase removed from the local operating-system credential store.",
            )])),
            Err(error) => Ok(call_error_result(error)),
        }
    }

    #[tool(
        description = "Remove the saved explicit-FTPS password for a host, port, and username from this machine's operating-system credential store. This does not alter any downloaded files or transfer history."
    )]
    pub(super) async fn remove_ftps_password(
        &self,
        Parameters(RemoveFtpsPasswordParams {
            host,
            port,
            username,
        }): Parameters<RemoveFtpsPasswordParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let port = port.unwrap_or(21);
        match downloads_call(self.downloads.clone(), true, move |client| {
            client.remove_ftps_password(&host, port, &username)
        })
        .await
        {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Saved explicit-FTPS password removed from the local operating-system credential store.",
            )])),
            Err(error) => Ok(call_error_result(error)),
        }
    }

    #[tool(
        description = "Get one transfer's current state: a one-sentence summary plus the full record -- state, bytes done and total, speed, ETA, per-segment progress, number of connections, \
        retries, the last error, whether the safety gatekeeper blocked it and why, whether pausing keeps its progress (resume_safe), and a log of recent events explaining what happened and why. \
        States: queued, awaiting_clearance (waiting for the gatekeeper's review), active, paused, completed, failed, cancelled, blocked."
    )]
    pub(super) async fn get_transfer(
        &self,
        Parameters(TransferIdParams { id }): Parameters<TransferIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(transfer_result(
            downloads_call(self.downloads.clone(), true, move |c| c.get(id)).await,
        ))
    }

    #[tool(
        description = "List every download transfer (oldest first) with a one-sentence summary each, optionally only those in one state (queued, awaiting_clearance, active, paused, completed, failed, cancelled, blocked)."
    )]
    pub(super) async fn list_transfers(
        &self,
        Parameters(ListTransfersParams { state }): Parameters<ListTransfersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let filter = match state.as_deref().map(parse_state).transpose() {
            Ok(filter) => filter,
            Err(message) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "invalid_request: {message}"
                ))]));
            }
        };
        match downloads_call(self.downloads.clone(), true, move |c| c.list(filter)).await {
            Ok(transfers) => Ok(CallToolResult::success(vec![Content::text(
                wrap_untrusted_transfer_content(
                    &serde_json::to_string_pretty(&transfer_list_json(&transfers))
                        .unwrap_or_else(|_| "{}".to_string()),
                ),
            )])),
            Err(error) => Ok(transfer_error_result(error)),
        }
    }

    #[tool(
        description = "Pause a queued or running transfer and wait until it has settled. Its progress is saved, and resume_transfer continues it -- unless its summary says the server gave nothing to resume from, in which case resuming starts again from the beginning."
    )]
    pub(super) async fn pause_transfer(
        &self,
        Parameters(TransferIdParams { id }): Parameters<TransferIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(transfer_result(
            downloads_call(self.downloads.clone(), false, move |c| c.pause(id)).await,
        ))
    }

    #[tool(
        description = "Resume a paused, failed, or blocked transfer. It goes through the safety gatekeeper's review again, so it can end up blocked. Returns immediately; poll with get_transfer."
    )]
    pub(super) async fn resume_transfer(
        &self,
        Parameters(TransferIdParams { id }): Parameters<TransferIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(transfer_result(
            downloads_call(self.downloads.clone(), false, move |c| c.resume(id)).await,
        ))
    }

    #[tool(
        description = "Cancel a transfer and delete its partial files. A completed transfer is left alone (its downloaded file is kept)."
    )]
    pub(super) async fn cancel_transfer(
        &self,
        Parameters(TransferIdParams { id }): Parameters<TransferIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(transfer_result(
            downloads_call(self.downloads.clone(), false, move |c| c.cancel(id)).await,
        ))
    }

    #[tool(
        description = "Remove a finished transfer (completed, failed, cancelled, or blocked) from the list. A running or paused transfer must be cancelled first. This never deletes a downloaded file."
    )]
    pub(super) async fn remove_transfer(
        &self,
        Parameters(TransferIdParams { id }): Parameters<TransferIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match downloads_call(self.downloads.clone(), false, move |c| c.remove(id)).await {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "transfer {id} removed from the list; a downloaded file, if any, was not deleted"
            ))])),
            Err(error) => Ok(call_error_result(error)),
        }
    }
}
