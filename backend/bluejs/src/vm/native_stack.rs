// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How much native stack the running thread has left.
//!
//! `Vm::enter_call` nests the Rust interpreter, so how deep JavaScript may
//! recurse is a question about the native stack (see
//! `completion::CALL_STACK_RED_ZONE`). This is a read-only query written in
//! Rust alone, with no assembly and no C: the current position is the address
//! of a local, and the stack's lower bound is asked of the OS once per thread.
//! It never switches or grows a stack.

use std::cell::Cell;

/// Bytes between the current position and the lowest address the OS lets this
/// thread use, or `None` where the host cannot report one.
pub(super) fn remaining_stack() -> Option<usize> {
    let lowest = LOWEST_STACK_ADDRESS.with(Cell::get)?;
    Some(current_stack_address().saturating_sub(lowest))
}

thread_local! {
    // Asking the OS can be slow (glibc reads `/proc/self/maps` for a main
    // thread), and a thread's stack never moves, so ask once per thread.
    static LOWEST_STACK_ADDRESS: Cell<Option<usize>> = Cell::new(lowest_stack_address());
}

// Stacks grow downwards on every supported target, so the address of a local
// is (approximately, to within this frame) how far the stack has grown.
#[inline(always)]
fn current_stack_address() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

#[cfg(target_os = "linux")]
fn lowest_stack_address() -> Option<usize> {
    // SAFETY: `attr` is initialised by `pthread_attr_init` before anything
    // reads it and destroyed on every path after that; the other calls only
    // write through the pointers passed to them.
    unsafe {
        let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
        if libc::pthread_attr_init(attr.as_mut_ptr()) != 0 {
            return None;
        }
        let mut address = std::ptr::null_mut();
        let mut size = 0;
        let found = libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()) == 0
            && libc::pthread_attr_getstack(attr.as_ptr(), &mut address, &mut size) == 0;
        libc::pthread_attr_destroy(attr.as_mut_ptr());
        found.then_some(address as usize)
    }
}

#[cfg(target_os = "macos")]
fn lowest_stack_address() -> Option<usize> {
    // SAFETY: both calls only read facts about the calling thread.
    unsafe {
        let thread = libc::pthread_self();
        // macOS reports the *highest* address of the stack, not the lowest.
        let base = libc::pthread_get_stackaddr_np(thread) as usize;
        base.checked_sub(libc::pthread_get_stacksize_np(thread))
    }
}

// The lowest pages of a Windows stack are the guard page and the reserve the
// OS keeps for handling an overflow, so nothing may run there; this pad stays
// well clear of both (the OS's default reserve is 16 KiB on 64-bit targets).
#[cfg(windows)]
const WINDOWS_UNUSABLE_LOW_STACK: usize = 64 * 1024;

#[cfg(windows)]
fn lowest_stack_address() -> Option<usize> {
    use windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits;
    let (mut low, mut high) = (0usize, 0usize);
    // SAFETY: writes only through the two pointers passed.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    (low != 0).then(|| low.saturating_add(WINDOWS_UNUSABLE_LOW_STACK))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn lowest_stack_address() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::remaining_stack;

    const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

    // Some hosts count the guard page as part of the reported extent (macOS on
    // Apple silicon reports about one 16 KiB page more than was requested), so
    // "no more than the stack given" is only true to within a few pages. The
    // call-depth guard's red zone is far larger than that.
    const REPORTING_SLACK: usize = 64 * 1024;

    fn on_thread_with_stack<R: Send + 'static>(
        bytes: usize,
        work: impl FnOnce() -> R + Send + 'static,
    ) -> R {
        std::thread::Builder::new()
            .stack_size(bytes)
            .spawn(work)
            .expect("spawn")
            .join()
            .expect("join")
    }

    #[inline(never)]
    fn remaining_below(frames: usize) -> usize {
        let padding = [0u8; 4096];
        std::hint::black_box(&padding);
        let remaining = if frames == 0 {
            remaining_stack().expect("supported platform")
        } else {
            remaining_below(frames - 1)
        };
        std::hint::black_box(&padding);
        remaining
    }

    #[test]
    fn reports_about_the_stack_the_thread_was_given() {
        if !SUPPORTED {
            assert_eq!(remaining_stack(), None);
            return;
        }
        const STACK: usize = 1024 * 1024;
        let remaining = on_thread_with_stack(STACK, || remaining_stack().expect("supported"));
        assert!(
            remaining <= STACK + REPORTING_SLACK,
            "{remaining} bytes on a {STACK}-byte stack"
        );
        // Almost all of a freshly spawned thread's stack is still unused.
        assert!(
            remaining > STACK / 2,
            "{remaining} bytes on a {STACK}-byte stack"
        );
    }

    #[test]
    fn shrinks_by_what_the_frames_in_between_use() {
        if !SUPPORTED {
            return;
        }
        let (shallow, deep) = on_thread_with_stack(4 * 1024 * 1024, || {
            (remaining_below(0), remaining_below(100))
        });
        // 100 frames each holding a 4 KiB array: at least ~400 KB deeper.
        assert!(
            shallow >= deep + 100 * 4096 * 9 / 10,
            "shallow {shallow}, deep {deep}"
        );
    }

    #[test]
    fn is_answered_per_thread() {
        if !SUPPORTED {
            return;
        }
        let small = on_thread_with_stack(256 * 1024, || remaining_stack().expect("supported"));
        let large = on_thread_with_stack(8 * 1024 * 1024, || remaining_stack().expect("supported"));
        assert!(small <= 256 * 1024 + REPORTING_SLACK, "{small}");
        assert!(large > 1024 * 1024, "{large}");
    }

    #[test]
    fn repeated_queries_on_one_thread_agree_to_within_a_frame() {
        if !SUPPORTED {
            return;
        }
        let (first, second) = on_thread_with_stack(1024 * 1024, || {
            (remaining_stack().unwrap(), remaining_stack().unwrap())
        });
        assert!(first.abs_diff(second) < 4096, "{first} vs {second}");
    }
}
