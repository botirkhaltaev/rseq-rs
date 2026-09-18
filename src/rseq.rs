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

/// Which rseq presence query to run. librseq `rseq_available`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Available {
    /// The `rseq` syscall exists (`EINVAL` on a null probe).
    Kernel,
    /// libc exports `__rseq_offset` / `__rseq_size` / `__rseq_flags`.
    Libc,
}

/// Process-wide rseq registration. `Copy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rseq {
    mode: Mode,
    cpus: Cpus,
    node: bool,
    cid: bool,
    slice: bool,
}

impl Rseq {
    /// glibc-registered area, or the crate's own area via `SYS_rseq`.
    /// `None` if the kernel has no rseq. `#[cold]`; call once.
    #[cold]
    #[must_use]
    pub fn new() -> Option<Self> {
        *STATE.get_or_init(Self::init)
    }

    /// librseq `rseq_available`. Does not require [`Self::new`].
    #[must_use]
    pub fn available(query: Available) -> bool {
        match query {
            Available::Kernel => registration::kernel_available(),
            Available::Libc => registration::libc_available(),
        }
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
        Some(Thread::new(area, self.node, self.cid, self.slice))
    }

    /// Expedited RSEQ membarrier targeted at `cpu`.
    /// Registers the command on first use.
    ///
    /// A cid word has no CPU. Draining one needs [`Self::fence_all`].
    /// After `fork`, registration is per-mm: the child may need a fresh
    /// register (retried once on `EPERM`).
    #[must_use]
    pub fn fence(self, cpu: CpuId) -> bool {
        cpu.get() < self.cpus.get() && Membarrier::fence(cpu)
    }

    /// Expedited RSEQ membarrier on every CPU. Use this to drain a cid word.
    ///
    /// After `fork`, see [`Self::fence`].
    #[must_use]
    pub fn fence_all(self) -> bool {
        Membarrier::fence_all()
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
        let (mode, slice) = if let Some((offset, size)) = registration::glibc_area_info() {
            // `slice_ctrl` is live only when both the kernel feature size and
            // this registration are large enough. Self-registered stays 32
            // bytes (legacy); glibc needs `__rseq_size >= 33`.
            let slice =
                registration::slice_supported() && (size as u64) >= crate::abi::SLICE_FEATURE_SIZE;
            (Mode::Glibc { offset }, slice)
        } else {
            Registration::bind()?;
            (Mode::SelfRegistered, false)
        };
        Some(Self {
            mode,
            cpus: Cpus::possible()?,
            node: registration::node_supported(),
            cid: registration::cid_supported(),
            slice,
        })
    }

    fn area(self) -> Option<NonNull<Area>> {
        match self.mode {
            Mode::Glibc { offset } => registration::glibc_area(offset),
            Mode::SelfRegistered => Registration::bind(),
        }
    }
}
