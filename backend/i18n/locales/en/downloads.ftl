# The about:downloads page. No message takes an argument: Fluent wraps a
# placeable in bidi-isolation marks, which would show as stray glyphs in
# page text, so the page composes values and labels itself.
downloads-title = Downloads
downloads-empty = No downloads yet.
downloads-unavailable = The downloads service is not running, so no downloads can be shown right now.
downloads-count-one = 1 download
downloads-count-other = downloads

state-queued = Queued
state-awaiting-clearance = Waiting for safety review
state-active = Downloading
state-paused = Paused
state-completed = Completed
state-failed = Failed
state-cancelled = Cancelled
state-blocked = Blocked
state-unknown = Unknown

label-of = of
label-total-unknown = total size unknown
label-speed = Speed:
label-eta = Time left:
label-connections = Connections:
label-mode = Mode:
label-retries = Retries:
label-last-error = Last error:
label-blocked = Blocked by the safety gatekeeper:
label-saved-to = Saved to:

mode-segmented = several connections at once
mode-single-stream = one connection (the server does not support parallel downloads)
mode-single-unknown-length = one connection (the server does not say how large the file is)
note-resume-unsafe = Pausing will restart this download from the beginning.
segments-heading = Progress of each connection
