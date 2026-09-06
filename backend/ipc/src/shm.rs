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
//! mid-read. `core` owns cleanup of the whole directory at shutdown --
//! there is no reference counting or explicit "done with this frame"
//! message from the client, an intentional MVP simplification (see
//! `phase-4-human-rendering-path/PLAN.md`).

use memmap2::{Mmap, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

/// Writes `bytes` (raw pixel data, row-major, format agreed out of
/// band -- BlueIce always uses RGBA8) to a new file for `generation`
/// under `dir`, via a real memory-mapped write.
pub fn write_frame(dir: &Path, generation: u64, bytes: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("frame-{generation}.rgba"));
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path)?;
    file.set_len(bytes.len() as u64)?;
    if !bytes.is_empty() {
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };
        mmap.copy_from_slice(bytes);
        mmap.flush()?;
    }
    Ok(path)
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
        let dir = std::env::temp_dir().join(format!("blueice-shm-test-missing-{}", std::process::id()));
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
        let dir = std::env::temp_dir().join(format!("blueice-shm-test-empty-{}", std::process::id()));
        let path = write_frame(&dir, 0, &[]).unwrap();
        let mapped = map_frame(&path).unwrap();
        assert_eq!(mapped.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mapping_a_nonexistent_path_is_an_error_not_a_panic() {
        let result = map_frame(Path::new("/nonexistent/blueice-frame.rgba"));
        assert!(result.is_err());
    }
}
