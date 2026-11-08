//! The core's allocator: image-sized blocks straight from the kernel.
//!
//! Apple's malloc keeps large blocks it has freed for reuse, and iPadOS still
//! counts them in the app's footprint, the number it ends apps by. Measured on
//! an M1 iPad Pro: a 540 MB block (one decoded 45 MP photo) freed left the
//! footprint at 553 MB. On the Mac, a full-size decode of a Leica Q2 left 2.5
//! GB counted after the photo, its renderer and every copy were dropped.
//!
//! Blocks of [`DIRECT`] or more are mapped with `mmap` and returned with
//! `munmap`, so freeing a photo frees its memory. Smaller ones, among them
//! the engine's per-frame tiles, stay with malloc: mapping them each frame
//! costs the kernel's page zeroing (Fit renders went from 8 to 11 ms with a
//! 1 MB threshold, 9 ms with 16 MB, on the Mac).

use std::alloc::{GlobalAlloc, Layout, System};

/// The smallest block mapped directly.
const DIRECT: usize = 16 << 20;

/// `mmap` returns page-aligned memory: 16 KB on Apple silicon, at least 4 KB
/// everywhere. Blocks asking for more alignment stay with malloc.
const MAPPED_ALIGN: usize = 4096;

pub(crate) struct Allocator;

fn direct(layout: Layout) -> bool {
    layout.size() >= DIRECT && layout.align() <= MAPPED_ALIGN
}

// SAFETY: each block is returned by the path that made it, decided by its
// layout alone, which Rust passes back unchanged; mapped memory is zeroed,
// page-aligned and at least `size` bytes, as `GlobalAlloc` requires.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !direct(layout) {
            // SAFETY: forwarded unchanged.
            return unsafe { System.alloc(layout) };
        }
        // SAFETY: an anonymous private mapping of `size` bytes.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                layout.size(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            std::ptr::null_mut()
        } else {
            p.cast()
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if direct(layout) {
            // SAFETY: as `alloc`; anonymous mappings are zero-filled.
            unsafe { self.alloc(layout) }
        } else {
            // SAFETY: forwarded unchanged.
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if direct(layout) {
            // SAFETY: `ptr` was mapped by `alloc` with this size.
            unsafe { libc::munmap(ptr.cast(), layout.size()) };
        } else {
            // SAFETY: forwarded unchanged.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `GlobalAlloc::realloc` requires a valid layout for `new_size`.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if !direct(layout) && !direct(new_layout) {
            // SAFETY: forwarded unchanged.
            return unsafe { System.realloc(ptr, layout, new_size) };
        }
        // Across the threshold, or between mappings: a new block and a copy.
        // SAFETY: `new_layout` is valid; the copy stays within both blocks.
        unsafe {
            let new = self.alloc(new_layout);
            if !new.is_null() {
                std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            new
        }
    }
}

#[cfg(test)]
mod tests {
    /// The process's footprint in MB, as iPadOS and macOS count it.
    #[cfg(target_os = "macos")]
    fn footprint_mb() -> u64 {
        // SAFETY: rusage_info_v4 is plain data; the call fills it for this
        // process.
        unsafe {
            let mut info: libc::rusage_info_v4 = std::mem::zeroed();
            libc::proc_pid_rusage(
                std::process::id() as i32,
                libc::RUSAGE_INFO_V4,
                (&mut info as *mut libc::rusage_info_v4).cast(),
            );
            info.ri_phys_footprint >> 20
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn a_freed_image_sized_block_leaves_the_footprint() {
        let before = footprint_mb();
        let block = vec![1u8; 256 << 20];
        assert!(footprint_mb() >= before + 200);
        drop(std::hint::black_box(block));
        assert!(
            footprint_mb() < before + 50,
            "{} MB left",
            footprint_mb() - before
        );
    }

    #[test]
    fn blocks_keep_their_contents_across_the_threshold() {
        let mut v: Vec<u32> = (0..1000).collect();
        // Grows from malloc into a mapping, then between mappings.
        v.resize(8 << 20, 7);
        v.resize(12 << 20, 9);
        assert_eq!(&v[..3], &[0, 1, 2]);
        assert_eq!(v[999], 999);
        assert_eq!(v[1000], 7);
        assert_eq!(v[(12 << 20) - 1], 9);
        v.truncate(10);
        v.shrink_to_fit();
        assert_eq!(v, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn zeroed_blocks_are_zero() {
        let v = vec![0u64; 4 << 20];
        assert!(v.iter().all(|&x| x == 0));
    }
}
