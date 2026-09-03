# Contributing

## Source file headers

Every source file must carry a header identifying its license and, where
applicable, its provenance. Which template applies depends on where the
file's content originated — see `development/browser_core/BROWSER_CORE_PLAN.md`
§2 for the licensing rationale behind each case.

There is no centralized `NOTICE`/`THIRD_PARTY_LICENSES` file — all
provenance and third-party notice text lives in the file header itself, so
a file's licensing history is self-contained and survives the file being
copied, moved, or extracted on its own.

### 1. Wholly original code

No prior-art relationship to Gecko or Chromium — this is the common case
for new files (the AI representation layer, project glue code, etc).

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
```

### 2. Files derived from Gecko

Staying MPL-2.0 is an inherent obligation here, not a choice — this
template just adds provenance on top of the same MPL header, for
traceability back to the upstream source.

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from Gecko: <path/in/gecko/tree> @ <upstream commit or revision>
```

### 3. Files derived from Chromium/Blink

BSD-3-Clause permits relicensing to MPL-2.0, but the original copyright
and disclaimer notice must be preserved verbatim. Copy the exact notice
block from the upstream file being ported — don't paraphrase it — and set
`<YEAR>` from that file's own copyright line, not from when it's ported.

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Portions of this file are derived from Chromium:
//   <path/in/chromium/tree> @ <upstream commit or revision>
//
// Copyright <YEAR> The Chromium Authors
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//    * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//    * Neither the name of Google LLC nor the names of its
// contributors may be used to endorse or promote products derived from
// this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

If the upstream Chromium/Blink file's own header text differs from the
block above (wording has shifted across Chromium revisions), preserve
*that file's actual header* rather than normalizing it to this template —
the requirement is to preserve the original notice, not to match this
example exactly.
