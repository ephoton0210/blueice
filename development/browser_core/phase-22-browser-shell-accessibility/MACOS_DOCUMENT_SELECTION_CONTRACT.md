# macOS document text selection

This milestone completes native selection/copy and parameterized AX text access
for ordinary rendered document text. It follows descendant live-region delivery
(`e48bbe914`). Implementation, complete native acceptance and Rust workspace
acceptance are complete. See [the dated results](MACOS_DOCUMENT_SELECTION_RESULTS.md).

Core builds a document-order text index from actual layout runs. Inline words and
soft wrapping retain the layout's space semantics; block boundaries become single
newlines. Grapheme geometry uses the same bundled font metrics as painting. Public
text excludes native controls and protected, hidden, inert or AX-hidden subtrees.
The frontend does not build a DOM, synthesize text or calculate selection layout.

The core retains a per-page anchor/cursor and paints selection. Pointer drag,
word/paragraph selection, Shift movement, Select All and Copy share that state.
Controls retain their own editing behavior. Document text is read-only: Cut,
Paste, replacement, composition and Undo/Redo cannot edit it or an unrelated
focused control. Mouse gestures over links defer ordinary activation until a
click completes, so a drag can select link text without navigation.

Native input uses the existing source/document/focus fence and ordered operation
queue. AX reads/selections additionally require the exact frame/node fence.
The completed click uses NativeClick through the existing reviewed click pipeline,
after queued selection acknowledgements and fresh input/viewport metadata. Mouse
events arriving while frame semantics or CSS dimensions are suspended are retained
for at most two seconds, then dispatched once. Tab/document/owner, view-size or
first-responder changes discard them; the bounded buffer coalesces drag updates.
Page-input keys also wait for the pointer's core focus acknowledgement, so native
editing shortcuts cannot vanish while local input state is suspended. During a
geometry gap they follow the original mouse events in the same bounded queue.
Global browser shortcuts continue normal window dispatch; cancelled transitions
discard queued page keys and expire after two seconds.
Paragraph/heading/link text callbacks expose UTF-16 ranges, grapheme and line
queries, public substrings and document-space geometry. Read queries preserve
DOM focus, selection and pixels. AX document selection preserves editor state;
ordinary focus changes return input ownership to the focused control.

Layout/scroll changes preserve an unchanged text/provenance selection and refresh
geometry. Public text/provenance changes, privacy transitions and navigation
discard the selection. Tabs keep independent state; window transfer fences old
input ownership. Session restoration never persists selected text.

Collection is bounded to 2,097,152 UTF-16 units and visited layout fragments,
4 MiB of JSON-encoded text within the 8 MiB IPC envelope,
256 ancestor levels and 65,536 selection paint rectangles. Omitted content is
reported through `limited`. Existing text-control limits remain unchanged.

Acceptance requires core regression tests, actual-service IPC, AppKit text/AX
callbacks, native-window XCUITest, full native and Rust validation, a dated result
record and a scoped commit/push. Physical VoiceOver and full bidirectional shaping
remain independently tracked browser delivery requirements.
