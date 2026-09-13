//! Expedited RSEQ membarrier. Registers the command on first use.

use std::sync::OnceLock;

use crate::thread::CpuId;

const PRIVATE_EXPEDITED_RSEQ: libc::c_int = 1 << 7;
const REGISTER_PRIVATE_EXPEDITED_RSEQ: libc::c_int = 1 << 8;
const FLAG_CPU: libc::c_int = 1 << 0;

static READY: OnceLock<bool> = OnceLock::new();

/// Process-wide RSEQ membarrier. Registers once; `fence` is a no-op if unavailable.
pub(crate) struct Membarrier;

impl Membarrier {
    fn ready() -> bool {
        *READY.get_or_init(|| {
            // SAFETY: `SYS_membarrier` takes (cmd, flags, cpuid). No memory operands.
            unsafe {
                libc::syscall(libc::SYS_membarrier, REGISTER_PRIVATE_EXPEDITED_RSEQ, 0, 0) == 0
            }
        })
    }

    /// Abort siblings' CS on `cpu`.
    pub(crate) fn fence(cpu: CpuId) -> bool {
        if !Self::ready() {
            return false;
        }
        let Ok(cpu) = libc::c_int::try_from(cpu.get()) else {
            return false;
        };
        // SAFETY: `SYS_membarrier` takes (cmd, flags, cpuid). No memory operands.
        unsafe { libc::syscall(libc::SYS_membarrier, PRIVATE_EXPEDITED_RSEQ, FLAG_CPU, cpu) == 0 }
    }
}
