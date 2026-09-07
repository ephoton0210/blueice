// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The built-in Help/About/Credits page (`phase-4-human-rendering-path/PLAN.md`'s
//! last checklist item). BSD-3-Clause's binary-distribution clause
//! requires Chromium's copyright notice, the redistribution
//! conditions, and the disclaimer to be reproduced "in the
//! documentation and/or other materials provided with the
//! distribution" -- the per-file source headers from Phase 0 cover
//! *source* distribution, not this. Gecko (MPL-2.0) and the bundled
//! DejaVu Sans font (Bitstream Vera license, `blueice-font`) are
//! credited alongside it for disclosure consistency, per Phase 0's
//! PLAN.md.
//!
//! Rendered through the normal HTML/CSS/layout/paint pipeline like any
//! other page -- reached by navigating to [`CREDITS_URL`] -- rather
//! than a separate hardcoded drawing path in `frontend`, so it goes
//! through the same one render pass `CLAUDE.md`'s core goal requires
//! for everything else.

/// The well-known URL [`Page::navigate`](crate::Page::navigate)
/// recognizes as a request for the built-in credits page instead of a
/// network fetch. `blueice-frontend` sends this in response to its
/// `credits` stdin command; it doesn't share this constant directly
/// (it's a different process, possibly not even Rust) -- like any
/// other URL, it's just a string carried over `ClientMessage::Navigate`.
pub const CREDITS_URL: &str = "about:credits";

/// The Chromium copyright notice, redistribution conditions, and
/// disclaimer, copied verbatim (content and wording, not line-by-line
/// formatting -- the MVP HTML/CSS subset has no `white-space: pre`
/// support, so the license block is reflowed into semantic
/// `<p>`/`<ul>` markup rather than a preformatted block) from
/// `development/browser_core/reference/chromium/LICENSE`.
pub const CREDITS_HTML: &str = r#"<html><head><title>About BlueIce</title></head><body>
<h1>About BlueIce</h1>
<p>BlueIce is a browser engine written from scratch in Rust.</p>

<h2>Technical references</h2>
<p>BlueIce is not a clean-room implementation: the Gecko (Firefox) and Chromium/Blink source trees are read directly as technical reference and a porting basis during development. This page credits both projects and reproduces the notice Chromium's license requires for binary distribution.</p>

<h2>Chromium</h2>
<p>Copyright 2015 The Chromium Authors</p>
<p>Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:</p>
<ul>
<li>Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.</li>
<li>Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.</li>
<li>Neither the name of Google LLC nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.</li>
</ul>
<p>THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.</p>

<h2>Mozilla Firefox (Gecko)</h2>
<p>Mozilla Firefox (Gecko) is licensed under the Mozilla Public License, v. 2.0. A copy of the license is available at https://mozilla.org/MPL/2.0/.</p>

<h2>Fonts</h2>
<p>This application bundles the DejaVu Sans font family, derived from Bitstream Vera.</p>
<p>Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream Vera is a trademark of Bitstream, Inc. DejaVu changes are in the public domain.</p>
<p>Permission is hereby granted, free of charge, to any person obtaining a copy of the fonts accompanying this license ("Fonts") and associated documentation files (the "Font Software"), to reproduce and distribute the Font Software, including without limitation the rights to use, copy, merge, publish, distribute, and/or sell copies of the Font Software, and to permit persons to whom the Font Software is furnished to do so, subject to the following conditions: the above copyright and trademark notices and this permission notice shall be included in all copies of one or more of the Font Software typefaces.</p>
</body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_url_uses_the_about_scheme() {
        assert_eq!(CREDITS_URL, "about:credits");
    }

    #[test]
    fn credits_html_reproduces_the_required_notices() {
        assert!(CREDITS_HTML.contains("Copyright 2015 The Chromium Authors"));
        assert!(CREDITS_HTML.contains("Redistributions of source code must retain"));
        assert!(CREDITS_HTML.contains("Redistributions in binary form must reproduce"));
        assert!(CREDITS_HTML.contains("THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS"));
        assert!(CREDITS_HTML.contains("Mozilla Public License"));
        assert!(CREDITS_HTML.contains("Bitstream"));
    }
}
