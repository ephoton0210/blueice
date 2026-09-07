# Phase 14 — i18n / Localization

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Done (for the UI text that existed when this phase started; see "Policy going forward" below for everything after)

## Objective

No UI-facing string in BlueIce should be a hardcoded literal in the crate that displays it — every one should go through a namespace/key lookup, ready to grow toward the language lists Windows/macOS ship (the eventual goal) without a redesign. This phase has two distinct halves, per how it was scoped when raised (mid-`Phase 4`/`Phase 2`/`Phase 1` work, not as its own planned phase from the start):

1. **Retrofit**: scan everything already implemented for hardcoded UI text and apply the new i18n mechanism to it. This phase's own job.
2. **Going forward**: every phase after this one that adds new UI-facing text applies i18n *at authoring time*, as part of that phase's own implementation — not deferred to a future retrofit. This phase exists so there's a real mechanism for later phases to use, not so it can absorb their i18n work too.

## What "UI-facing text" means here

Audited everything shipped by Phase 0-4 (Phases 5-13 haven't produced any UI text yet — Phase 5's AI-facing representation is data, not display strings; Phases 6-13 are unimplemented). Two real hits, both retrofitted:

- `blueice_engine::credits`'s Help/About/Credits page content (Phase 4's last checklist item).
- `blueice-frontend-reference`'s window title (`"BlueIce (reference frontend)"` / `"BlueIce -- {url}"`).

**Explicitly not in scope**: `eprintln!`-based diagnostic messages in `blueice-frontend-reference`/`blueice-core` (e.g. "failed to send {msg:?}", "core reported an error") — these are developer-facing stderr logs for debugging a reference implementation, not end-user chrome, the same distinction real browsers draw between a crash log and a dialog box. Localizing them would suggest they're meant for the person using the browser, which they aren't.

## Architecture (decided)

**Backed by Fluent (`fluent-bundle` + `unic-langid`)** — the same localization system Gecko/Firefox itself uses, reused for the same reason `blueice-net` uses `ureq` and `blueice-frontend-reference` uses `winit`: message formatting (plural rules, argument substitution, and eventually bidi) is solved infrastructure, not the parsing/layout/paint domain this project is written from scratch for (`CLAUDE.md`). A hand-rolled string table would have needed its own rewrite once plural rules or RTL scripts entered the picture — the exact rewrite-prone-architecture trap `research/js-bytecode-eventloop.md` already argues against elsewhere in this project.

**New crate: `backend/i18n` (`blueice-i18n`)**. One function, `translate(locale, namespace, key, args) -> String`, backed by `.ftl` resource files embedded via `include_str!` under `locales/<locale>/<namespace>.ftl` (namespaces so far: `credits`, `frontend`, one per crate that owns UI text). Falls back to `DEFAULT_LOCALE` ("en") whenever the requested locale has no resource for a namespace *or* is simply missing one key — a partially-translated locale must never render blank text, only English text, for the keys it hasn't gotten to yet. Fluent's bidi-isolation marks (FSI/PDI around substituted arguments) are left on rather than disabled, since that's exactly the safety net an embedded URL/name needs inside a future RTL (Arabic, Hebrew) translation.

**Two locales shipped today**: `en` (the fallback, must define every key) and `zh-TW` (Traditional Chinese). `SUPPORTED_LOCALES` is a two-entry list today, meant to grow.

**Legal-notice text policy (decided)**: the Chromium BSD-3-Clause notice and the DejaVu/Bitstream Vera notice on the credits page are always rendered in English first — that's the literal text BSD-3-Clause's "reproduce the above copyright notice... verbatim" requirement refers to, and a translation could be read as not satisfying that requirement. A locale's own translation of the same notice is appended underneath, introduced by a `translation-notice` string that explicitly labels it non-authoritative. Ordinary prose (headings, descriptive paragraphs, the Gecko credit line) doesn't get this treatment — it's just translated.

**Locale selection**: the credits page takes an explicit `?lang=<locale>` query parameter on its `about:credits` URL (`blueice_engine::credits::locale_from_url`), defaulting to `en` if absent or unsupported. `blueice-frontend-reference` detects a default locale from the `$LANG` environment variable at startup (`detect_locale`, normalizing POSIX `zh_TW.UTF-8`-shaped values to the `zh-TW` tag `blueice-i18n` keys resources by) — explicitly a stand-in for a real platform-native frontend reading its OS's own locale API (Windows' `GetUserDefaultLocaleName`, macOS's `NSLocale`), the same relationship stdin's `show`/`hide`/`credits` commands already have to a real AI-facing control channel. No IPC protocol change was needed for any of this — locale is carried as an ordinary URL parameter and an ordinary environment variable, not a new `ClientMessage` variant.

## A real bug the retrofit found: CJK glyph coverage

Rendering the new zh-TW credits page to PNG for manual verification (this phase's own "actually look at it" check, same bar as Phase 4's) showed every Chinese character as blank space — DejaVu Sans (`blueice-font`'s only bundled face before this phase) carries essentially no CJK glyphs. The text was correctly generated and laid out (line heights matched real advance widths, so `blueice-layout`'s measurement wasn't wrong), it just never got painted.

**Fix**: bundled Noto Sans TC Regular (SIL Open Font License, `backend/core/font/assets/NotoSansTC-LICENSE.txt`) as a fallback-only face. `blueice_font::font_for_char(c, bold, italic)` checks the requested DejaVu face first (`Font::has_glyph`) and only consults the CJK face for a codepoint DejaVu can't cover; `measure_text_width` and `blueice-raster`'s glyph-drawing loop were both changed to resolve the font *per character* rather than once per run, since a run can legitimately mix scripts (an English link inside a Chinese sentence). Only one CJK weight is bundled — bold/italic Chinese text still falls back to the single regular-weight face rather than going blank again, a deliberate, documented simplification, not a second missing-glyph bug reappearing one level down. This touched three crates Phase 3 had already marked Done (`blueice-font`, `blueice-raster`, and indirectly `blueice-layout` via the shared measurement function) — a real, if small, instance of a "Done" phase needing to reopen for a correctness bug a later phase's manual-verification step actually caught, exactly the kind of thing `TEST_PLAN.md`'s "look at it, don't just trust the number" philosophy exists to catch.

## Recorded, not designed: cross-platform font management

Raised mid-phase: whatever font-selection mechanism BlueIce ends up with should not paint itself into a corner once real platform-native frontends (SwiftUI/AppKit on Apple platforms, PySide6/Qt elsewhere, per Phase 4's plan) exist. **Deliberately not designed here** — per `CLAUDE.md`'s own rule against designing for hypothetical future requirements, and because there is nothing to design against yet: per Phase 4's architecture, `core` performs *all* rasterization and ships finished pixels to `frontend` over the frame-plane, so a native frontend only blits pixels — it does not do its own text layout or font selection for page content. The place this could eventually matter is native OS *chrome* (a platform-native About dialog, window titles rendered by the OS toolkit itself rather than fetched through `core`), which doesn't exist anywhere in this repo today — no SwiftUI/AppKit/PySide6 frontend has been started. Recorded here as a note for whichever future phase actually adds a second, platform-native frontend, rather than built speculatively now.

## Policy going forward

Every phase after this one that introduces new UI-facing text (a new `about:` page, a future settings/preferences surface, a future native frontend's own chrome) adds it through `blueice-i18n` — a new namespace and `en` key at minimum — as part of that phase's own implementation, the same way it's expected to write tests as part of implementing a feature rather than bolting them on after (`CLAUDE.md`'s Definition of Done). This phase's checklist ends once the Phase 0-4 retrofit is done; it does not track future phases' i18n work item-by-item.

## Checklist

- [x] Decide the i18n architecture (Fluent-backed namespace/key lookup, `en` fallback policy, legal-text-stays-English-plus-translation rule) — see "Architecture (decided)" above
- [x] Stand up `blueice-i18n` (`translate`, `SUPPORTED_LOCALES`, `DEFAULT_LOCALE`), with `en` and `zh-TW` resources for every namespace
- [x] Audit already-implemented UI text and retrofit it — `blueice_engine::credits` (localized, legal text dual-rendered) and `blueice-frontend-reference`'s window title (`$LANG`-detected locale)
- [x] Find and fix the CJK-glyph-coverage gap the retrofit surfaced — Noto Sans TC fallback face in `blueice-font`, per-character font resolution in `blueice-raster`/`measure_text_width`
- [x] Record the cross-platform (SwiftUI/AppKit/PySide6) font-management consideration for a future native-frontend phase, without designing it now
- [x] State the going-forward policy for future phases' own UI text — see "Policy going forward" above

All checklist items for the Phase 0-4 retrofit are done. `blueice-i18n` is ready for `SUPPORTED_LOCALES` to grow past two entries whenever a specific new locale is actually needed, without another architecture change.
