//! Per-thread 32-byte `struct rseq` when glibc did not register one.

use core::{cell::UnsafeCell, mem::size_of, ptr::NonNull};

use crate::abi::{
    AT_RSEQ_FEATURE_SIZE, Area, CID_FEATURE_SIZE, CPU_REG_FAILED, CPU_UNINIT, FLAG_UNREGISTER,
    GLIBC_SIZE_MIN, NODE_FEATURE_SIZE, SIG, SLICE_FEATURE_SIZE,
};

thread_local! {
    static SLOT: Registration = const { Registration::new() };
}

/// Crate-owned rseq TLS. Registers on first [`Self::bind`]; unregisters in `Drop`.
#[repr(C, align(32))]
pub(crate) struct Registration {
    area: UnsafeCell<Area>,
}

impl Registration {
    const fn new() -> Self {
        Self {
            area: UnsafeCell::new(Area {
                cpu_id_start: 0,
                cpu_id: CPU_UNINIT,
                rseq_cs: 0,
                flags: 0,
                node_id: 0,
                mm_cid: 0,
                slice_ctrl: 0,
            }),
        }
    }

    /// Register this thread's area if needed. `None` if `SYS_rseq` fails.
    pub(crate) fn bind() -> Option<NonNull<Area>> {
        SLOT.try_with(Self::register).ok().flatten()
    }

    fn register(&self) -> Option<NonNull<Area>> {
        let ptr = self.area.get();
        // SAFETY: `area` is this thread's TLS `struct rseq`; the kernel writes `cpu_id`.
        let id = unsafe { core::ptr::addr_of!((*ptr).cpu_id).read_volatile() };
        if id != CPU_UNINIT && id != CPU_REG_FAILED {
            return NonNull::new(ptr);
        }
        // SAFETY: `SYS_rseq` takes (rseq, len, flags, sig). `ptr` is 32-byte aligned
        // live TLS of `size_of::<Area>()` bytes. No other registrant on this thread.
        let rc = unsafe { libc::syscall(libc::SYS_rseq, ptr, size_of::<Area>(), 0, SIG) };
        if rc == 0 { NonNull::new(ptr) } else { None }
    }
}

/// glibc TLS offset and reported `__rseq_size`. `None` if glibc did not
/// register (size below the legacy minimum).
pub(crate) fn glibc_area_info() -> Option<(isize, usize)> {
    // SAFETY: `dlsym` looks up exported glibc symbols. Null means absent.
    let size_p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_size".as_ptr()) };
    // SAFETY: same as `size_p`; `__rseq_offset` is an exported ptrdiff_t.
    let offset_p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_offset".as_ptr()) };
    if size_p.is_null() || offset_p.is_null() {
        return None;
    }
    // SAFETY: glibc publishes `__rseq_size` as `unsigned int`.
    let size = usize::try_from(unsafe { size_p.cast::<libc::c_uint>().read() }).ok()?;
    if size < GLIBC_SIZE_MIN {
        return None;
    }
    // SAFETY: glibc publishes `__rseq_offset` as `ptrdiff_t`.
    let offset = unsafe { offset_p.cast::<libc::ptrdiff_t>().read() };
    Some((offset, size))
}

pub(crate) fn node_supported() -> bool {
    // SAFETY: `getauxval` looks up an auxv entry; 0 if the type is absent.
    unsafe { libc::getauxval(AT_RSEQ_FEATURE_SIZE) >= NODE_FEATURE_SIZE }
}

pub(crate) fn cid_supported() -> bool {
    // SAFETY: `getauxval` looks up an auxv entry; 0 if the type is absent.
    unsafe { libc::getauxval(AT_RSEQ_FEATURE_SIZE) >= CID_FEATURE_SIZE }
}

/// Kernel advertises `slice_ctrl` via auxv. The registration must also be
/// large enough ([`SLICE_FEATURE_SIZE`]); see [`crate::rseq::Rseq::init`].
pub(crate) fn slice_supported() -> bool {
    // SAFETY: `getauxval` looks up an auxv entry; 0 if the type is absent.
    unsafe { libc::getauxval(AT_RSEQ_FEATURE_SIZE) >= SLICE_FEATURE_SIZE }
}

/// `SYS_rseq(NULL, 0, 0, 0)` is `EINVAL` when the syscall exists, `ENOSYS` if not.
pub(crate) fn kernel_available() -> bool {
    // SAFETY: null probe; the kernel rejects it without touching memory.
    let rc = unsafe { libc::syscall(libc::SYS_rseq, core::ptr::null::<u8>(), 0, 0, 0) };
    rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL)
}

/// libc exports the three rseq TLS symbols.
pub(crate) fn libc_available() -> bool {
    // SAFETY: `dlsym` looks up exported glibc symbols. Null means absent.
    unsafe {
        !libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_size".as_ptr()).is_null()
            && !libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_offset".as_ptr()).is_null()
            && !libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_flags".as_ptr()).is_null()
    }
}

pub(crate) fn glibc_area(offset: isize) -> Option<NonNull<Area>> {
    #[cfg(target_arch = "x86_64")]
    {
        let tp: *mut u8;
        // SAFETY: `fs:0` is the x86_64 thread pointer.
        unsafe {
            core::arch::asm!(
                "mov {}, fs:0",
                out(reg) tp,
                options(nostack, preserves_flags, readonly, pure)
            );
        }
        NonNull::new(tp.wrapping_offset(offset).cast())
    }
    #[cfg(target_arch = "aarch64")]
    {
        let tp: *mut u8;
        // SAFETY: `tpidr_el0` is the aarch64 thread pointer.
        unsafe {
            core::arch::asm!(
                "mrs {}, tpidr_el0",
                out(reg) tp,
                options(nostack, preserves_flags, readonly, pure)
            );
        }
        NonNull::new(tp.wrapping_offset(offset).cast())
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let ptr = self.area.get();
        // SAFETY: TLS dtor runs before the block is freed. Ignore errors:
        // never-registered and already-unregistered both fail the syscall.
        unsafe {
            libc::syscall(libc::SYS_rseq, ptr, size_of::<Area>(), FLAG_UNREGISTER, SIG);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Registration;
    use core::mem::{align_of, size_of};

    #[test]
    fn own_abi() {
        assert_eq!(size_of::<Registration>(), 32);
        assert_eq!(align_of::<Registration>(), 32);
    }
}
