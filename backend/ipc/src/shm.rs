// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The frame-plane: how a rasterized frame's pixel bytes actually
//! reach a client, referenced by [`crate::ServerMessage::FrameReady`]'s
//! `shm_path`. Per `research/frontend-ipc.md` §4, BlueIce's raster path
//! is CPU-side (no GPU handle to hand across a process boundary the
//! way Chromium's `Mailbox`/Gecko's `SurfaceDescriptor` do), so this
//! implements the "shared memory as the universal fallback" leg both
//! reference engines also fall back to for software compositing --
//! real `mmap`, not a buffered read/write through the socket the
//! control-plane messages travel over.
//!
//! Each frame generation gets its own file rather than one file
//! mutated in place: a client that has already mapped generation N
//! must never observe a torn write from generation N+1 landing
//! mid-read. There is no reference counting or explicit "done with
//! this frame" message from the client -- instead, [`write_frame`]
//! prunes generations older than [`RETAINED_GENERATIONS`] every time a
//! new one is written, rather than only at shutdown (an earlier version
//! of this module did exactly that, an architectural-risk-survey
//! finding: a long-running interactive session, precisely this
//! project's target use case, would otherwise accumulate one file per
//! render forever). Deleting a file a client already has open/mmap'd
//! is always safe on the POSIX targets this project runs on today (the
//! data stays reachable through that existing handle until the client
//! itself unmaps or closes it, per `unlink(2)`); the retention window
//! exists only so a client that has *received* a `FrameReady` but not
//! yet gotten around to opening that path doesn't find it already gone.

use memmap2::{Mmap, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

/// How many of the most recent generations' frame files to keep on disk
/// at once. Small enough to keep disk usage bounded regardless of
/// session length, generous enough that a client which has received a
/// `FrameReady` but not immediately opened it (a few renders behind, not
/// stalled indefinitely) still finds its file there.
const RETAINED_GENERATIONS: u64 = 4;

/// Writes `bytes` (raw pixel data, row-major, format agreed out of
/// band -- BlueIce always uses RGBA8) to a new file for `generation`
/// under `dir`, via a real memory-mapped write, then prunes whichever
/// single generation has just aged out of [`RETAINED_GENERATIONS`].
pub fn write_frame(dir: &Path, generation: u64, bytes: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("frame-{generation}.rgba"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    file.set_len(bytes.len() as u64)?;
    if !bytes.is_empty() {
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };
        mmap.copy_from_slice(bytes);
        mmap.flush()?;
    }
    prune_expired_generation(dir, generation);
    Ok(path)
}

/// Removes the one generation's file that just fell outside the
/// retention window, if any -- not a scan over every older generation,
/// since every prior call already pruned everything before it (and a
/// missing file here, e.g. right after startup, is simply ignored).
fn prune_expired_generation(dir: &Path, current_generation: u64) {
    if let Some(expired) = current_generation.checked_sub(RETAINED_GENERATIONS) {
        let _ = std::fs::remove_file(dir.join(format!("frame-{expired}.rgba")));
    }
}

/// Maps a frame written by [`write_frame`] read-only. Returns the raw
/// `Mmap` (derefs to `&[u8]`) rather than copying into a `Vec` --
/// callers that need to reinterpret the bytes (e.g. `frontend`
/// repacking RGBA8 into its own pixel format) can iterate the mapping
/// directly.
pub fn map_frame(path: &Path) -> io::Result<Mmap> {
    let file = OpenOptions::new().read(true).open(path)?;
    Ok(unsafe { MmapOptions::new().map(&file)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_map_round_trips_the_bytes() {
        let dir = std::env::temp_dir().join(format!("blueice-shm-test-{}", std::process::id()));
        let bytes = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let path = write_frame(&dir, 0, &bytes).unwrap();
        let mapped = map_frame(&path).unwrap();
        assert_eq!(&mapped[..], &bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_frame_creates_the_directory_if_it_does_not_exist_yet() {
        let dir =
            std::env::temp_dir().join(format!("blueice-shm-test-missing-{}", std::process::id()));
        assert!(!dir.exists());
        write_frame(&dir, 0, &[9, 9, 9]).unwrap();
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_generation_gets_its_own_file_so_older_generations_stay_readable() {
        let dir = std::env::temp_dir().join(format!("blueice-shm-test-gen-{}", std::process::id()));
        let path1 = write_frame(&dir, 1, &[1]).unwrap();
        let path2 = write_frame(&dir, 2, &[2]).unwrap();
        assert_ne!(path1, path2);
        assert_eq!(&map_frame(&path1).unwrap()[..], &[1]);
        assert_eq!(&map_frame(&path2).unwrap()[..], &[2]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_an_empty_frame_does_not_panic_and_maps_to_an_empty_slice() {
        let dir =
            std::env::temp_dir().join(format!("blueice-shm-test-empty-{}", std::process::id()));
        let path = write_frame(&dir, 0, &[]).unwrap();
        let mapped = map_frame(&path).unwrap();
        assert_eq!(mapped.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frames_older_than_the_retention_window_are_deleted() {
        let dir =
            std::env::temp_dir().join(format!("blueice-shm-test-prune-{}", std::process::id()));
        for gen in 1..=RETAINED_GENERATIONS {
            write_frame(&dir, gen, &[gen as u8]).unwrap();
        }
        // Still within the window: nothing pruned yet.
        assert!(dir.join("frame-1.rgba").exists());

        // One more generation pushes generation 1 out of the window.
        let latest = write_frame(&dir, RETAINED_GENERATIONS + 1, &[99]).unwrap();
        assert!(
            !dir.join("frame-1.rgba").exists(),
            "generation 1 must be pruned once it's outside the retention window"
        );
        assert!(
            dir.join("frame-2.rgba").exists(),
            "generation 2 is still within the window"
        );
        assert_eq!(
            &map_frame(&latest).unwrap()[..],
            &[99],
            "the just-written generation must still be fully readable"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_a_generation_whose_file_is_already_gone_does_not_error() {
        // write_frame must not panic/fail just because an earlier
        // generation's file was already removed (or never existed --
        // e.g. right after a restart with a fresh, mostly-empty
        // frame directory).
        let dir = std::env::temp_dir().join(format!(
            "blueice-shm-test-prune-missing-{}",
            std::process::id()
        ));
        let result = write_frame(&dir, RETAINED_GENERATIONS + 5, &[1]);
        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mapping_a_nonexistent_path_is_an_error_not_a_panic() {
        let result = map_frame(Path::new("/nonexistent/blueice-frame.rgba"));
        assert!(result.is_err());
    }
}
