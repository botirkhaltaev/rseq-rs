use core::ptr::NonNull;
use std::sync::OnceLock;

use crate::abi::{Area, CPU_REG_FAILED, CPU_UNINIT};
use crate::cpus::Cpus;
use crate::membarrier::Membarrier;
use crate::registration::{self, Registration};
use crate::thread::{CpuId, Thread};
use crate::words::Words;

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
    cid: bool,
}

impl Rseq {
    /// glibc-registered area, or the crate's own area via `SYS_rseq`.
    /// `None` if the kernel has no rseq. `#[cold]`; call once.
    #[cold]
    #[must_use]
    pub fn new() -> Option<Self> {
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
        Some(Thread::new(area, self.cid))
    }

    /// Expedited RSEQ membarrier targeted at `cpu`.
    /// Registers the command on first use.
    ///
    /// A cid word has no CPU. Draining one needs a fence on every CPU (or
    /// an un-targeted membarrier). `fence_all` is a later item.
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
        let mode = if let Some(offset) = registration::glibc_offset() {
            Mode::Glibc { offset }
        } else {
            Registration::bind()?;
            Mode::SelfRegistered
        };
        Some(Self {
            mode,
            cpus: Cpus::possible()?,
            cid: registration::cid_supported(),
        })
    }

    fn area(self) -> Option<NonNull<Area>> {
        match self.mode {
            Mode::Glibc { offset } => registration::glibc_area(offset),
            Mode::SelfRegistered => Registration::bind(),
        }
    }
}
