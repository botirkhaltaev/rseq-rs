use core::ptr::NonNull;
use std::sync::OnceLock;

use crate::{
    abi::{AREA_MIN, Area, CPU_REG_FAILED, CPU_UNINIT},
    cpus::Cpus,
    membarrier::Membarrier,
    thread::{CpuId, Thread},
    words::Words,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::registration::Registration;

static STATE: OnceLock<Option<Rseq>> = OnceLock::new();

/// Who registered this process's per-thread `struct rseq`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Glibc { offset: isize },
    SelfRegistered,
}

/// Process-wide rseq registration. `Copy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rseq {
    mode: Mode,
    cpus: Cpus,
}

impl Rseq {
    /// glibc-registered area, or the crate's own area via `SYS_rseq`.
    /// `None` if the kernel has no rseq. `#[cold]`; call once.
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
            let mode = if let Some(offset) = Self::glibc_offset() {
                Mode::Glibc { offset }
            } else {
                Registration::bind()?;
                Mode::SelfRegistered
            };
            Some(Self {
                mode,
                cpus: Cpus::possible()?,
            })
        }
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn glibc_offset() -> Option<isize> {
        // SAFETY: `dlsym` looks up exported glibc symbols. Null means absent.
        let size_p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_size".as_ptr()) };
        let offset_p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"__rseq_offset".as_ptr()) };
        if size_p.is_null() || offset_p.is_null() {
            return None;
        }
        // SAFETY: glibc publishes `__rseq_size` as `unsigned int`.
        let size = usize::try_from(unsafe { size_p.cast::<libc::c_uint>().read() }).ok()?;
        if size < AREA_MIN {
            return None;
        }
        // SAFETY: glibc publishes `__rseq_offset` as `ptrdiff_t`.
        Some(unsafe { offset_p.cast::<libc::ptrdiff_t>().read() })
    }

    fn area(self) -> Option<NonNull<Area>> {
        match self.mode {
            Mode::Glibc { offset } => Self::glibc_area(offset),
            Mode::SelfRegistered => {
                #[cfg(all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                ))]
                {
                    Registration::bind()
                }
                #[cfg(not(all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )))]
                {
                    None
                }
            }
        }
    }

    fn glibc_area(offset: isize) -> Option<NonNull<Area>> {
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
        {
            let _ = offset;
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
            NonNull::new(tp.wrapping_offset(offset).cast())
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
            NonNull::new(tp.wrapping_offset(offset).cast())
        }
    }
}
