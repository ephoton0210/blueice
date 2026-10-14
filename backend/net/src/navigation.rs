// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Closed navigation methods. POST bytes stay private and are erased when
//! their last navigation/history owner drops them; Debug never prints them.

use crate::FetchError;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroizing;

pub const MAX_FORM_BODY_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormEncoding {
    UrlEncoded,
    PlainText,
    Multipart { boundary: String },
}

impl FormEncoding {
    fn content_type(&self) -> Result<String, FetchError> {
        Ok(match self {
            Self::UrlEncoded => "application/x-www-form-urlencoded".into(),
            Self::PlainText => "text/plain".into(),
            Self::Multipart { boundary } => {
                if boundary.is_empty()
                    || boundary.len() > 70
                    || !boundary
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                {
                    return Err(FetchError::Request("invalid multipart boundary".into()));
                }
                format!("multipart/form-data; boundary={boundary}")
            }
        })
    }
}

#[derive(Clone)]
pub struct NavigationRequest {
    url: String,
    post: Option<PostData>,
}

#[derive(Clone)]
struct PostData {
    content_type: String,
    body: Arc<Zeroizing<Vec<u8>>>,
}

impl NavigationRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            post: None,
        }
    }

    pub fn post(
        url: impl Into<String>,
        encoding: FormEncoding,
        body: Vec<u8>,
    ) -> Result<Self, FetchError> {
        let body = Zeroizing::new(body);
        if body.len() > MAX_FORM_BODY_BYTES {
            return Err(FetchError::Request(
                "form data exceeds the navigation limit".into(),
            ));
        }
        Ok(Self {
            url: url.into(),
            post: Some(PostData {
                content_type: encoding.content_type()?,
                body: Arc::new(body),
            }),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn method(&self) -> &'static str {
        if self.post.is_some() {
            "POST"
        } else {
            "GET"
        }
    }
    pub fn content_type(&self) -> Option<&str> {
        self.post.as_ref().map(|post| post.content_type.as_str())
    }
    pub fn body(&self) -> &[u8] {
        self.post.as_ref().map_or(&[], |post| post.body.as_slice())
    }

    /// Declarative same-origin rewrites retain the method. The caller must
    /// review this new target before the network client is invoked.
    pub fn retarget(&mut self, url: String) {
        self.url = url;
    }

    /// A 301/302 POST or 303 response becomes GET. A 307/308 keeps the body;
    /// no automatic second request occurs in this method.
    pub fn follow_redirect(&mut self, url: String, status: u16) {
        if status == 303 || self.post.is_some() && matches!(status, 301 | 302) {
            self.post = None;
        }
        self.url = url;
    }
}

impl fmt::Debug for NavigationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NavigationRequest")
            .field("url", &self.url)
            .field("method", &self.method())
            .field("body_bytes", &self.body().len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for NavigationRequest {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.method() == other.method()
            && self.content_type() == other.content_type()
            && self.body() == other.body()
    }
}
impl Eq for NavigationRequest {}
