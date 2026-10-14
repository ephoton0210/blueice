# macOS download folder selection

Status: implemented and accepted as a scoped increment. Actual native and fresh
Rust workspace acceptance are recorded in [the results](MACOS_DOWNLOAD_FOLDER_RESULTS.md).
Base commit: `2ad46935cac13195d1550e66a77f0007dc60d708`.

Downloads > Folder opens a SwiftUI configuration surface. An AppKit NSOpenPanel
selects one readable and writable local directory, rejecting leaf symlinks,
aliases, files and nonlocal URLs. Selection and Use default edit the draft.
Only Apply writes the versioned path preferences. Cancelling a picker or
dismissing the surface discards its draft.

Applying checkpoints and reaps the owned download service, invalidates prior
credential reviews and starts its replacement without restarting the browser
core. Transfers keep their IDs, original canonical folders, files and partials.
Unfinished transfers remain paused through relaunch until explicit Resume.
New transfers use the newly selected directory. Completed files in retained
folders continue to pass native Open/Finder and quarantine checks.

The Rust store records each transfer's original root, including transfers
whose filename has not yet been assigned. Legacy version 1 records acquire
their current root during the first normal manager open. Stored roots are
provenance, not authorization: the process owner must also supply historical
folders through bounded `--previous-download-dir` startup arguments. Neither
the downloads protocol nor AI tools can change that set. An unapproved stored
root refuses startup before the catalog is rewritten. It never changes a
completed record into a failure or redirects a transfer into the new folder.

At most 32 historical folders are retained. Existing folder permissions are
preserved. Directories newly created by the download manager use mode 0700;
its private catalog retains the existing private-directory policy. The picker
does not move completed files or download partials between folders.

Malformed or future native settings remain stored and refuse service startup
until an explicit repair. If repair omits a recorded original folder, startup
refuses without altering the catalog. Explicitly choosing that original folder
authorizes it and recovers its history before selecting the new destination.
Historical folders must remain available; otherwise
startup refuses before rewriting the catalog. The native surface reports this
condition and requires restoring the original location. This increment does
not implement unavailable-volume recovery or automatic migration of files.

Acceptance requires actual local HTTP bytes before/after a folder change,
paused recovery and explicit resume into the original directory, new downloads
in the selected directory, Finder access to old files, unchanged selected-folder
permissions, cancel/draft tests, English and Traditional Chinese screenshots,
the complete native suite, fresh Rust checks and scoped source/product/process
audits. Verification images remain ignored by Git. The complete macOS browser
goal remains in progress.
