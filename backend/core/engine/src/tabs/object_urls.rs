// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(crate) enum BlobNavigationKind {
    New,
    Reload,
    History(HistoryDirection),
}
impl TabManager {
    pub(crate) fn apply_blob_navigation(
        &mut self,
        tab: TabId,
        url: &str,
        lease: &crate::ObjectUrlLease,
        kind: BlobNavigationKind,
    ) -> Result<(), String> {
        let mime = lease
            .mime_type()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let text = String::from_utf8_lossy(lease.bytes());
        let html = match mime {
            "text/html" => text.into_owned(),
            "" | "text/plain" | "application/json" => format!(
                "<pre>{}</pre>",
                text.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            ),
            _ => return Err("Unsupported Blob document MIME type".into()),
        };
        let mut next = self.new_history_page(tab);
        next.load_html_str(&html, Some(url.into()));
        next.object_url_origin = Some(lease.origin_identity());
        match kind {
            BlobNavigationKind::New => self.replace_current_as_new_navigation(tab, next),
            BlobNavigationKind::Reload => {
                let previous = self
                    .tabs
                    .get_mut(&tab)
                    .ok_or("Unknown Blob navigation tab")?;
                next.continue_tab_generations_from(&previous.page);
                previous.page = next;
                previous.document_epoch = previous
                    .document_epoch
                    .checked_add(1)
                    .expect("document identity exhausted");
            }
            BlobNavigationKind::History(direction) => {
                if !self.replace_current_from_history(tab, direction, next) {
                    return Err("Blob history destination unavailable".into());
                }
            }
        }
        Ok(())
    }
}
