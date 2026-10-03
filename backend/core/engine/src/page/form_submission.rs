// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Successful controls are derived from the current core document. The
//! frontend supplies a native activation, never a replacement entry list.

use super::native_interaction::{form_owner, input_type, tag};
use super::*;
use crate::navigation_request::{BrowserNavigation, FormSource};
use blueice_net::{FormEncoding, NavigationRequest, MAX_FORM_BODY_BYTES};

struct Entry<'a> {
    name: String,
    value: String,
    empty_file: bool,
    file: Option<&'a blueice_ipc::file_input::FileData>,
}

fn crlf(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

fn attribute(doc: &Document, node: NodeId, name: &str) -> String {
    element_attribute(doc, node, name)
        .unwrap_or_default()
        .into()
}

impl Page {
    pub(super) fn native_form_controls(&self, form: NodeId) -> Vec<NodeId> {
        let mut controls = Vec::new();
        let mut pending = vec![self.doc.root()];
        while let Some(node) = pending.pop() {
            pending.extend(
                self.doc
                    .children(node)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
            if matches!(
                tag(&self.doc, node),
                "input" | "button" | "textarea" | "select"
            ) && form_owner(&self.doc, node) == Some(form)
            {
                controls.push(node);
            }
        }
        controls
    }

    pub(crate) fn native_implicit_form(
        &self,
        action: &blueice_ipc::input::TextInputAction,
    ) -> Option<NodeId> {
        use blueice_ipc::input::{PageKey, TextInputAction};
        if !matches!(
            action,
            TextInputAction::Key {
                key: PageKey::Enter,
                ..
            }
        ) {
            return None;
        }
        if self.submission_pending {
            return None;
        }
        let focused = self.focused?;
        let blocks = |node| {
            tag(&self.doc, node) == "input"
                && matches!(
                    input_type(&self.doc, node).as_str(),
                    "text"
                        | "search"
                        | "url"
                        | "tel"
                        | "email"
                        | "password"
                        | "date"
                        | "month"
                        | "week"
                        | "time"
                        | "datetime-local"
                        | "number"
                )
        };
        if !self.native_focusable(focused) || !blocks(focused) {
            return None;
        }
        let form = form_owner(&self.doc, focused)?;
        let controls = self.native_form_controls(form);
        if controls.iter().any(|node| {
            tag(&self.doc, *node) == "button"
                && !matches!(input_type(&self.doc, *node).as_str(), "button" | "reset")
                || tag(&self.doc, *node) == "input"
                    && matches!(input_type(&self.doc, *node).as_str(), "submit" | "image")
        }) {
            return None;
        }
        (controls.into_iter().filter(|node| blocks(*node)).count() <= 1).then_some(form)
    }

    fn selected_options(&self, node: NodeId) -> Vec<NodeId> {
        blueice_layout::select_options(&self.doc, node)
            .into_iter()
            .filter(|option| option.selected)
            .map(|option| option.node)
            .collect()
    }
    fn select_display_size(&self, node: NodeId) -> u32 {
        blueice_layout::select_display_size(&self.doc, node)
    }
    fn submitted_options(&self, node: NodeId) -> Vec<NodeId> {
        self.selected_options(node)
            .into_iter()
            .filter(|option| !self.native_control_disabled(*option))
            .collect()
    }
    fn option_submission_value(&self, node: NodeId) -> String {
        element_attribute(&self.doc, node, "value")
            .map(str::to_string)
            .unwrap_or_else(|| {
                node_text_content(&self.doc, node)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
    }

    pub(crate) fn prepare_native_form_submission(
        &self,
        form: NodeId,
        submitter: Option<NodeId>,
    ) -> Result<BrowserNavigation, String> {
        let document_url = self.url.as_ref().ok_or("A form needs a document URL")?;
        let chosen = |name: &str, override_name: &str| {
            submitter
                .and_then(|node| element_attribute(&self.doc, node, override_name))
                .or_else(|| element_attribute(&self.doc, form, name))
        };
        let method = chosen("method", "formmethod")
            .unwrap_or("get")
            .to_ascii_lowercase();
        if method == "dialog" {
            return Err("Dialog form submission is unavailable".into());
        }
        let post = method == "post";
        let action = chosen("action", "formaction")
            .filter(|action| !action.is_empty())
            .unwrap_or(document_url);
        let mut base = Url::parse(document_url).map_err(|_| "Invalid form action")?;
        let mut nodes = vec![self.doc.root()];
        while let Some(node) = nodes.pop() {
            nodes.extend(
                self.doc
                    .children(node)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
            if tag(&self.doc, node) == "base" {
                if let Some(href) = element_attribute(&self.doc, node, "href") {
                    if let Ok(resolved) = base.join(href) {
                        base = resolved;
                    }
                    break;
                }
            }
        }
        let mut url = base.join(action).map_err(|_| "Invalid form action")?;
        blueice_net::validate_url_scheme(url.as_str()).map_err(|error| error.to_string())?;
        let controls = self.native_form_controls(form);
        let validate = element_attribute(&self.doc, form, "novalidate").is_none()
            && submitter
                .is_none_or(|node| element_attribute(&self.doc, node, "formnovalidate").is_none());
        let encoding = chosen("enctype", "formenctype")
            .unwrap_or("application/x-www-form-urlencoded")
            .to_ascii_lowercase();
        let mut entries = Vec::new();
        let mut protected = false;
        let mut size = 0usize;
        for node in controls {
            if self.native_control_disabled(node) {
                continue;
            }
            let mut ancestor = self.doc.parent(node);
            let mut in_datalist = false;
            while let Some(parent) = ancestor {
                if tag(&self.doc, parent) == "datalist" {
                    in_datalist = true;
                    break;
                }
                ancestor = self.doc.parent(parent);
            }
            if in_datalist {
                continue;
            }
            let kind = tag(&self.doc, node);
            let input = input_type(&self.doc, node);
            let value = match kind {
                "input" if input == "range" => {
                    self.native_control_public_value(node).unwrap_or_default()
                }
                "textarea" => node_text_content(&self.doc, node),
                "select" => self.native_select_value(node).unwrap_or_default(),
                _ => attribute(&self.doc, node, "value"),
            };
            let readonly = element_attribute(&self.doc, node, "readonly").is_some()
                && (kind == "textarea"
                    || kind == "input"
                        && matches!(
                            input.as_str(),
                            "text"
                                | "search"
                                | "url"
                                | "tel"
                                | "email"
                                | "password"
                                | "date"
                                | "month"
                                | "week"
                                | "time"
                                | "datetime-local"
                                | "number"
                        ));
            let validates = kind == "textarea"
                || kind == "select"
                || kind == "input"
                    && !matches!(
                        input.as_str(),
                        "hidden" | "button" | "submit" | "reset" | "image" | "range" | "color"
                    );
            if validate
                && validates
                && !readonly
                && element_attribute(&self.doc, node, "required").is_some()
            {
                let missing = if kind == "input" && input == "checkbox" {
                    element_attribute(&self.doc, node, "checked").is_none()
                } else if kind == "input" && input == "radio" {
                    !self
                        .native_radios(node)
                        .into_iter()
                        .any(|member| element_attribute(&self.doc, member, "checked").is_some())
                } else if kind == "input" && input == "file" {
                    self.native_files.get(&node).is_none_or(Vec::is_empty)
                } else if kind == "select" {
                    let selected = self.selected_options(node);
                    selected.is_empty()
                        || selected.len() == 1
                            && element_attribute(&self.doc, node, "multiple").is_none()
                            && self.select_display_size(node) == 1
                            && self.native_options(node).first() == selected.first()
                            && self.doc.parent(selected[0]) == Some(node)
                            && self.option_submission_value(selected[0]).is_empty()
                } else {
                    value.is_empty()
                };
                if missing {
                    return Err("Please fill out the required form field".into());
                }
            }
            let name = attribute(&self.doc, node, "name");
            if name.is_empty() {
                continue;
            }
            let submit_control = kind == "button"
                || kind == "input"
                    && matches!(input.as_str(), "button" | "submit" | "reset" | "image");
            if submit_control
                && (submitter != Some(node)
                    || matches!(input.as_str(), "reset" | "button" | "image"))
            {
                continue;
            }
            if kind == "input"
                && matches!(input.as_str(), "checkbox" | "radio")
                && element_attribute(&self.doc, node, "checked").is_none()
            {
                continue;
            }
            if kind == "select" {
                for option in self.submitted_options(node) {
                    let value = self.option_submission_value(option);
                    size = size.saturating_add(name.len()).saturating_add(value.len());
                    entries.push(Entry {
                        name: name.clone(),
                        value,
                        empty_file: false,
                        file: None,
                    });
                    if entries.len() > 1024 || size > MAX_FORM_BODY_BYTES {
                        return Err("Form data exceeds the navigation limit".into());
                    }
                }
            } else if kind == "input"
                && input == "file"
                && self.native_files.get(&node).is_some_and(|f| !f.is_empty())
            {
                for file in &self.native_files[&node] {
                    size = size
                        .saturating_add(name.len())
                        .saturating_add(file.name.len())
                        .saturating_add(if post && encoding == "multipart/form-data" {
                            file.bytes.len()
                        } else {
                            0
                        });
                    entries.push(Entry {
                        name: name.clone(),
                        value: file.name.clone(),
                        empty_file: false,
                        file: Some(file),
                    });
                    if entries.len() > 1024 || size > MAX_FORM_BODY_BYTES {
                        return Err("Form data exceeds the navigation limit".into());
                    }
                }
            } else {
                let empty_file = kind == "input" && input == "file";
                let value = if empty_file {
                    String::new()
                } else if kind == "input" && input == "hidden" && name == "_charset_" {
                    "UTF-8".into()
                } else if kind == "input"
                    && matches!(input.as_str(), "checkbox" | "radio")
                    && element_attribute(&self.doc, node, "value").is_none()
                {
                    "on".into()
                } else {
                    value
                };
                protected |= kind == "input"
                    && (matches!(input.as_str(), "password" | "credit-card" | "payment")
                        || attribute(&self.doc, node, "autocomplete")
                            .split_ascii_whitespace()
                            .any(|word| {
                                word.to_ascii_lowercase().starts_with("cc-")
                                    || word.eq_ignore_ascii_case("one-time-code")
                            }));
                size = size.saturating_add(name.len()).saturating_add(value.len());
                entries.push(Entry {
                    name,
                    value,
                    empty_file,
                    file: None,
                });
            }
            if entries.len() > 1024 || size > MAX_FORM_BODY_BYTES {
                return Err("Form data exceeds the navigation limit".into());
            }
        }
        // Never encode a protected field into a GET URL, including review,
        // trace and history metadata. POST receives mandatory per-hop review.
        if protected && !post {
            return Err("Protected form fields require a POST request".into());
        }
        let request = if !post || encoding != "text/plain" && encoding != "multipart/form-data" {
            let mut encoded = String::new();
            for entry in &entries {
                let pair = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair(&crlf(&entry.name), &crlf(&entry.value))
                    .finish();
                if encoded
                    .len()
                    .saturating_add(pair.len())
                    .saturating_add(usize::from(!encoded.is_empty()))
                    > MAX_FORM_BODY_BYTES
                {
                    return Err("Form data exceeds the navigation limit".into());
                }
                if !encoded.is_empty() {
                    encoded.push('&');
                }
                encoded.push_str(&pair);
            }
            if post {
                NavigationRequest::post(
                    url.to_string(),
                    FormEncoding::UrlEncoded,
                    encoded.into_bytes(),
                )
            } else {
                url.set_query(Some(&encoded));
                Ok(NavigationRequest::get(url.to_string()))
            }
        } else if encoding == "text/plain" {
            let mut encoded = String::new();
            for entry in &entries {
                encoded.push_str(&format!("{}={}\r\n", crlf(&entry.name), crlf(&entry.value)));
                if encoded.len() > MAX_FORM_BODY_BYTES {
                    return Err("Form data exceeds the navigation limit".into());
                }
            }
            NavigationRequest::post(
                url.to_string(),
                FormEncoding::PlainText,
                encoded.into_bytes(),
            )
        } else {
            let mut nonce = [0_u8; 16];
            getrandom::fill(&mut nonce).map_err(|_| "Cannot create a multipart boundary")?;
            let boundary = format!(
                "BlueIce{}",
                nonce
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            let mut encoded = Vec::new();
            for entry in &entries {
                let escape = |value: &str| {
                    crlf(value)
                        .replace('\r', "%0D")
                        .replace('\n', "%0A")
                        .replace('"', "%22")
                };
                let name = escape(&entry.name);
                encoded.extend_from_slice(
                    format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"")
                        .as_bytes(),
                );
                if let Some(file) = entry.file {
                    encoded.extend_from_slice(
                        format!(
                            "; filename=\"{}\"\r\nContent-Type: {}",
                            escape(&file.name),
                            file.media_type
                        )
                        .as_bytes(),
                    );
                } else if entry.empty_file {
                    encoded.extend_from_slice(
                        b"; filename=\"\"\r\nContent-Type: application/octet-stream",
                    );
                }
                encoded.extend_from_slice(b"\r\n\r\n");
                if let Some(file) = entry.file {
                    encoded.extend_from_slice(&file.bytes);
                } else {
                    encoded.extend_from_slice(crlf(&entry.value).as_bytes());
                }
                encoded.extend_from_slice(b"\r\n");
                if encoded.len() > MAX_FORM_BODY_BYTES {
                    return Err("Form data exceeds the navigation limit".into());
                }
            }
            encoded.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            NavigationRequest::post(
                url.to_string(),
                FormEncoding::Multipart { boundary },
                encoded,
            )
        }
        .map_err(|error| error.to_string())?;
        Ok(BrowserNavigation {
            request,
            form: Some(FormSource {
                document_url: document_url.clone(),
                has_protected_fields: protected,
            }),
        })
    }
}
