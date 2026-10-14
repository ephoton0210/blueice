# The about:assistant page. No message takes an argument: Fluent wraps a
# placeable in bidi-isolation marks, which would show as stray glyphs in page
# text, so the page composes values and labels itself.
assistant-title = Assistant
assistant-empty = Nothing here yet. Ask for a summary of a page, or ask to organize its data, and the result will appear here.
assistant-unavailable = No local assistant is configured, so summaries and organized data are not available.
assistant-note = Written by a local model from the page's text. Check important details against the page itself.
kind-summary = Summary
kind-organized = Organized data
label-source = Page:
label-request = Request:
label-failed = Could not complete this request:

# The about:assistant settings section: no message takes an argument (see the
# header of assistant.ftl), so the page composes labels and values itself.
settings-heading = Settings
settings-none = No settings file was given, so the assistant is configured only by its start-up options.
settings-missing = The settings file does not exist, so no assistant is configured.
settings-invalid = The settings file could not be used:
settings-file = File:
settings-apply = Changes to this file take effect when the launcher next starts.
settings-backend = Backend:
backend-none = None (no assistant)
backend-loopback = Loopback server
backend-candle = In-process (candle)
backend-both = Both at once (double the resources)
settings-loopback = Loopback model:
settings-candle-model = Candle model:
settings-candle-tokenizer = Candle tokenizer:
settings-candle-context = Candle context (tokens):
settings-idle = Idle timeout (seconds):
settings-memory = Memory ceiling (MiB):
settings-memory-none = No limit
settings-nice = Priority (nice):
