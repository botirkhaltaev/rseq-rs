//! Per-thread 32-byte `struct rseq` when glibc did not register one.

use core::{cell::UnsafeCell, ptr::NonNull};

use crate::abi::{AREA_OWN, Area, CPU_REG_FAILED, CPU_UNINIT, FLAG_UNREGISTER, SIG};

thread_local! {
    static SLOT: Registration = const { Registration::new() };
}

/// Crate-owned rseq TLS. Registers on first [`Self::bind`]; unregisters in `Drop`.
#[repr(C, align(32))]
pub(crate) struct Registration {
    area: UnsafeCell<Area>,
    _tail: [u32; 3],
}

impl Registration {
    const fn new() -> Self {
        Self {
            area: UnsafeCell::new(Area {
                cpu_id_start: 0,
                cpu_id: CPU_UNINIT,
                rseq_cs: 0,
                flags: 0,
            }),
            _tail: [0; 3],
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
        // live TLS of `AREA_OWN` bytes. No other registrant on this thread.
        let rc = unsafe { libc::syscall(libc::SYS_rseq, ptr, AREA_OWN, 0, SIG) };
        if rc == 0 { NonNull::new(ptr) } else { None }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let ptr = self.area.get();
        // SAFETY: TLS dtor runs before the block is freed. Ignore errors:
        // never-registered and already-unregistered both fail the syscall.
        unsafe {
            libc::syscall(libc::SYS_rseq, ptr, AREA_OWN, FLAG_UNREGISTER, SIG);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Registration;
    use crate::abi::AREA_OWN;
    use core::mem::{align_of, size_of};

    #[test]
    fn own_abi() {
        assert_eq!(size_of::<Registration>(), AREA_OWN);
        assert_eq!(align_of::<Registration>(), AREA_OWN);
    }
}
