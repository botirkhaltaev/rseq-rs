use core::ptr::NonNull;
use std::sync::OnceLock;

use crate::{
    abi::{AREA_MIN, Area, CPU_REG_FAILED, CPU_UNINIT},
    cpus::Cpus,
    membarrier::Membarrier,
    thread::{CpuId, Thread},
    words::Words,
};

static STATE: OnceLock<Option<Rseq>> = OnceLock::new();

/// Process-wide rseq registration. `Copy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rseq {
    offset: isize,
    cpus: Cpus,
}

impl Rseq {
    /// glibc-registered area and CPU count.
    /// `None` if rseq is unavailable. `#[cold]`; call once.
    #[cold]
    #[must_use]
    pub fn try_new() -> Option<Self> {
        *STATE.get_or_init(Self::init)
    }

    /// Bind this thread's area. `#[cold]`; store the `Thread` in caller TLS.
    #[cold]
    #[must_use]
    pub fn bind(self) -> Option<Thread> {
        let area = self.area()?;
        // SAFETY: `area` is a registered rseq TLS; the kernel writes `cpu_id`.
        let id = unsafe { core::ptr::addr_of!((*area.as_ptr()).cpu_id).read_volatile() };
        if id == CPU_UNINIT || id == CPU_REG_FAILED {
            return None;
        }
        Some(Thread::new(area))
    }

    /// Expedited RSEQ membarrier targeted at `cpu`.
    /// Registers the command on first use.
    #[must_use]
    pub fn fence(self, cpu: CpuId) -> bool {
        cpu.get() < self.cpus.get() && Membarrier::fence(cpu)
    }

    /// CPU count used to size the per-CPU word region.
    #[must_use]
    pub const fn cpus(self) -> u32 {
        self.cpus.get()
    }

    /// Map a new per-CPU word region sized for [`Self::cpus`]. Caller owns it.
    #[must_use]
    pub fn words(self) -> Option<Words> {
        Words::new(self.cpus.get())
    }

    fn init() -> Option<Self> {
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            return None;
        }
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            // SAFETY: glibc publishes `__rseq_size` (0 if rseq is off).
            let size = usize::try_from(unsafe { __rseq_size }).ok()?;
            if size < AREA_MIN {
                return None;
            }
            // SAFETY: glibc publishes `__rseq_offset` for every thread.
            let offset = unsafe { __rseq_offset };
            Some(Self {
                offset,
                cpus: Cpus::possible()?,
            })
        }
    }

    fn area(self) -> Option<NonNull<Area>> {
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            let _ = self;
            None
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
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
            NonNull::new(tp.wrapping_offset(self.offset).cast())
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
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
            NonNull::new(tp.wrapping_offset(self.offset).cast())
        }
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
unsafe extern "C" {
    static __rseq_offset: libc::ptrdiff_t;
    static __rseq_size: libc::c_uint;
}
