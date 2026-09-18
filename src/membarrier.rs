//! Expedited RSEQ membarrier. Registers the command on first use.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::thread::CpuId;

const FLAG_CPU: libc::c_int = 1 << 0;

static READY: OnceLock<bool> = OnceLock::new();
/// Set after a successful fence so a post-fork `EPERM` can force one re-register.
static USED: AtomicBool = AtomicBool::new(false);

/// Process-wide RSEQ membarrier. Registers once; `fence` is a no-op if unavailable.
pub(crate) struct Membarrier;

impl Membarrier {
    fn register() -> bool {
        // SAFETY: `SYS_membarrier` takes (cmd, flags, cpuid). No memory operands.
        unsafe {
            libc::syscall(
                libc::SYS_membarrier,
                libc::MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
                0,
                0,
            ) == 0
        }
    }

    fn ready() -> bool {
        *READY.get_or_init(Self::register)
    }

    /// After `fork`, the child's `mm` needs its own register. `READY` may still
    /// say true from the parent; retry once on `EPERM`.
    fn run(cmd: libc::c_int, flags: libc::c_int, cpu: libc::c_int) -> bool {
        if !Self::ready() {
            return false;
        }
        // SAFETY: `SYS_membarrier` takes (cmd, flags, cpuid). No memory operands.
        let rc = unsafe { libc::syscall(libc::SYS_membarrier, cmd, flags, cpu) };
        if rc == 0 {
            USED.store(true, Ordering::Relaxed);
            return true;
        }
        let err = std::io::Error::last_os_error().raw_os_error();
        if err == Some(libc::EPERM) && USED.swap(false, Ordering::Relaxed) && Self::register() {
            // SAFETY: same as above; one re-register after fork.
            return unsafe { libc::syscall(libc::SYS_membarrier, cmd, flags, cpu) } == 0;
        }
        false
    }

    /// Abort siblings' CS on `cpu`.
    pub(crate) fn fence(cpu: CpuId) -> bool {
        let Ok(cpu) = libc::c_int::try_from(cpu.get()) else {
            return false;
        };
        Self::run(libc::MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ, FLAG_CPU, cpu)
    }

    /// Abort siblings' CS on every CPU.
    pub(crate) fn fence_all() -> bool {
        Self::run(libc::MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ, 0, 0)
    }
}
