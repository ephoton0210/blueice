// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use super::*;
use blueice_ipc::navigation_session::{NavigationEntry, NavigationHistory};

fn from_page(page: &Page) -> NavigationEntry {
    NavigationEntry {
        url: page.url().map(str::to_string),
        was_post: page.post_expired
            || page
                .last_navigation
                .as_ref()
                .is_some_and(|nav| nav.request.method() == "POST"),
    }
}
fn from_entry(entry: &HistoryEntry) -> NavigationEntry {
    match entry {
        HistoryEntry::Reload { url, was_post, .. } => NavigationEntry {
            url: url.clone(),
            was_post: *was_post,
        },
        HistoryEntry::Snapshot(page) => from_page(page),
    }
}
fn validate(history: &NavigationHistory) -> Result<(), &'static str> {
    history.validate()?;
    for entry in &history.entries {
        let Some(url) = &entry.url else { continue };
        if matches!(
            url.as_str(),
            "about:credits" | "about:settings" | "about:assistant"
        ) || crate::downloads_page::is_downloads_url(url)
        {
            if entry.was_post {
                return Err("Built-in pages cannot restore POST history");
            }
            continue;
        }
        let parsed = Url::parse(url).map_err(|_| "Invalid saved navigation URL")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Saved navigation URL is unavailable");
        }
    }
    Ok(())
}
impl TabManager {
    pub(crate) fn navigation_session(&self, id: TabId) -> Result<NavigationHistory, &'static str> {
        let tab = self.tabs.get(&id).ok_or("Unknown session tab")?;
        let mut entries: Vec<_> = tab.back.iter().map(from_entry).collect();
        let cursor = entries.len();
        entries.push(from_page(&tab.page));
        entries.extend(tab.forward.iter().rev().map(from_entry));
        let history = NavigationHistory {
            entries,
            cursor,
            zoom: tab.page.page_zoom,
        };
        validate(&history)?;
        Ok(history)
    }
    /// Imported GET metadata cannot manufacture a displayed, unreviewed page.
    /// POST metadata is an expired marker: no request body survives restoration.
    pub(crate) fn restore_navigation_session(
        &mut self,
        id: TabId,
        history: NavigationHistory,
    ) -> Result<bool, &'static str> {
        validate(&history)?;
        let tab = self.tabs.get_mut(&id).ok_or("Unknown session tab")?;
        let current = &history.entries[history.cursor];
        if current.was_post {
            if tab.page.url().is_some() {
                return Err("Restore POST history into a blank tab");
            }
            tab.page.load_html_str("<h1>POST page could not be restored</h1><p>Submit the original form again. Private form data was not saved.</p>",current.url.clone());
            tab.page.post_expired = true;
            tab.document_epoch = tab
                .document_epoch
                .checked_add(1)
                .expect("document identity exhausted");
        } else if from_page(&tab.page) != *current {
            return Err("Saved history does not match the reviewed current document");
        }
        let record = |entry: &NavigationEntry| HistoryEntry::Reload {
            url: entry.url.clone(),
            navigation: None,
            was_post: entry.was_post,
        };
        tab.back = history.entries[..history.cursor]
            .iter()
            .map(record)
            .collect();
        tab.forward = history.entries[history.cursor + 1..]
            .iter()
            .rev()
            .map(record)
            .collect();
        self.set_page_zoom(id, history.zoom);
        Ok(current.was_post)
    }
}
