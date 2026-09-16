//! Page-rounded mmap of one `AtomicUsize` per CPU, or a caller-owned span.

use core::{mem::size_of, num::NonZeroUsize, ptr::NonNull, sync::atomic::AtomicUsize};

const PAGE: usize = 4096;

/// Caller-owned or crate-mapped word region.
pub(crate) enum Region {
    Mapped {
        base: NonNull<u8>,
        len: NonZeroUsize,
    },
    Raw(NonNull<u8>),
}

impl Region {
    pub(crate) fn map(cpus: u32) -> Option<Self> {
        let bytes = usize::try_from(cpus)
            .ok()?
            .checked_mul(size_of::<AtomicUsize>())?;
        let len = NonZeroUsize::new(bytes.div_ceil(PAGE).checked_mul(PAGE)?)?;
        // SAFETY: anonymous private mapping, page-rounded length.
        let ptr = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                len.get(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        Some(Self::Mapped {
            base: NonNull::new(ptr.cast())?,
            len,
        })
    }

    pub(crate) const fn raw(base: NonNull<u8>) -> Self {
        Self::Raw(base)
    }

    pub(crate) const fn base(&self) -> NonNull<u8> {
        match self {
            Self::Mapped { base, .. } | Self::Raw(base) => *base,
        }
    }

    /// Word at `base + cpu * size_of::<AtomicUsize>()`.
    ///
    /// # Safety
    /// `cpu` is in range and this region is live aligned words.
    pub(crate) unsafe fn word(&self, cpu: u32) -> NonNull<AtomicUsize> {
        let offset = (cpu as usize).wrapping_mul(size_of::<AtomicUsize>());
        // SAFETY: `cpu` is in range; mmap of usizes is aligned.
        unsafe { NonNull::new_unchecked(self.base().as_ptr().add(offset).cast()) }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        if let Self::Mapped { base, len } = self {
            // SAFETY: `Region::Mapped` uniquely owns this mmap.
            unsafe {
                libc::munmap(base.as_ptr().cast(), len.get());
            }
        }
    }
}
