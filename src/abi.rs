//! Kernel `struct rseq`. Every registration is 32 bytes; `__rseq_size` is a
//! feature report, not the mapped size.

/// Full uapi `struct rseq`. `repr(C)`: `rseq_cs` at 8 gives align 8, size 32.
#[repr(C)]
pub(crate) struct Area {
    pub cpu_id_start: u32,
    pub cpu_id: u32,
    pub rseq_cs: u64,
    pub flags: u32,
    pub node_id: u32,
    pub mm_cid: u32,
    pub slice_ctrl: u32,
}

/// Smallest `__rseq_size` glibc reports when it registered an area.
pub(crate) const GLIBC_SIZE_MIN: usize = 20;
/// `rseq(2)` `RSEQ_FLAG_UNREGISTER`.
pub(crate) const FLAG_UNREGISTER: u32 = 1;
pub(crate) const CPU_UNINIT: u32 = u32::MAX;
/// `RSEQ_CPU_ID_REGISTRATION_FAILED` (`-2`).
pub(crate) const CPU_REG_FAILED: u32 = u32::MAX - 1;
/// glibc `RSEQ_SIG` on x86-64.
#[cfg(target_arch = "x86_64")]
pub(crate) const SIG: u32 = 0x5305_3053;
/// glibc `RSEQ_SIG_CODE` on little-endian aarch64 (`BRK #0x45E0`).
#[cfg(target_arch = "aarch64")]
pub(crate) const SIG: u32 = 0xd428_bc00;
pub(crate) const CPU_ID_OFF: usize = 4;
pub(crate) const CS_OFF: usize = 8;
pub(crate) const MM_CID_OFF: usize = 24;
/// `AT_RSEQ_FEATURE_SIZE` auxv type.
pub(crate) const AT_RSEQ_FEATURE_SIZE: libc::c_ulong = 27;
/// `offsetofend(struct rseq, node_id)`. Kernel populates `node_id` at this size.
pub(crate) const NODE_FEATURE_SIZE: libc::c_ulong = 24;
/// `offsetofend(struct rseq, mm_cid)`. Kernel populates `mm_cid` at this size.
pub(crate) const CID_FEATURE_SIZE: libc::c_ulong = 28;
/// `offsetofend(struct rseq, slice_ctrl)`. Kernel populates `slice_ctrl` at this size.
pub(crate) const SLICE_FEATURE_SIZE: libc::c_ulong = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn area_abi() {
        assert_eq!(offset_of!(Area, cpu_id_start), 0);
        assert_eq!(offset_of!(Area, cpu_id), 4);
        assert_eq!(offset_of!(Area, rseq_cs), 8);
        assert_eq!(offset_of!(Area, flags), 16);
        assert_eq!(offset_of!(Area, node_id), 20);
        assert_eq!(offset_of!(Area, mm_cid), 24);
        assert_eq!(offset_of!(Area, slice_ctrl), 28);
        assert_eq!(size_of::<Area>(), 32);
        assert_eq!(GLIBC_SIZE_MIN, 20);
        assert_eq!(FLAG_UNREGISTER, 1);
        #[cfg(target_arch = "x86_64")]
        assert_eq!(SIG, 0x5305_3053);
        #[cfg(target_arch = "aarch64")]
        assert_eq!(SIG, 0xd428_bc00);
        assert_eq!(CPU_ID_OFF, 4);
        assert_eq!(CS_OFF, 8);
        assert_eq!(MM_CID_OFF, 24);
        assert_eq!(AT_RSEQ_FEATURE_SIZE, 27);
        assert_eq!(NODE_FEATURE_SIZE, 24);
        assert_eq!(CID_FEATURE_SIZE, 28);
        assert_eq!(SLICE_FEATURE_SIZE, 32);
    }
}
