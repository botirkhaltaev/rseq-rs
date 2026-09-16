use core::fmt::{self, Display};
use core::ptr::{self, NonNull};

use crate::abi::{Area, CPU_ID_OFF, CPU_REG_FAILED, CPU_UNINIT, MM_CID_OFF};
use crate::attempt::{Attempt, Memcpy};
use crate::cs;
use crate::words::Word;

/// Longest memcpy [`Thread::store_if_copy`] will perform inside the CS.
pub const COPY_MAX: usize = 64;

/// Compare-miss or an index-mismatch abort. Kernel preemption restarts inside the CS.
///
/// Exhaustive on purpose: the CS has two outcomes. Do not mark `non_exhaustive`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The word was not `expect`. Carries the value that was seen.
    Miss(usize),
    /// This thread's index is not `word.key`. Re-read [`Thread::cpu_id`] or
    /// [`Thread::cid`] and pick a new word. Do not retry the same [`Word`].
    Abort,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Miss(seen) => write!(f, "compare miss: found {seen}"),
            Self::Abort => f.write_str("index mismatch"),
        }
    }
}

impl std::error::Error for Error {}

mod private {
    /// Crate-internal: offset and numeric id for [`super::Index`] kinds.
    pub trait Sealed: Copy + Eq {
        const OFF: usize;
        fn get(self) -> u32;
    }
}

/// Per-thread index the CS confirms: [`CpuId`] or [`Cid`].
///
/// Sealed. Downstream crates can name this trait as a bound but cannot
/// implement it.
pub trait Index: private::Sealed {}

/// Logical CPU index. Newtype so it cannot be mixed with a raw `u32`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CpuId(u32);

impl CpuId {
    /// Rejects the kernel uninitialized and registration-failed sentinels.
    #[must_use]
    pub const fn new(id: u32) -> Option<Self> {
        if id == CPU_UNINIT || id == CPU_REG_FAILED {
            None
        } else {
            Some(Self(id))
        }
    }

    /// The numeric CPU id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl private::Sealed for CpuId {
    const OFF: usize = CPU_ID_OFF;

    fn get(self) -> u32 {
        self.0
    }
}

impl Index for CpuId {}

/// Compact concurrency id (`mm_cid`). Dense in `[0, min(threads, allowed CPUs))`.
///
/// Obtainable only from [`Thread::cid`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Cid(u32);

impl Cid {
    pub(crate) const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The numeric concurrency id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl private::Sealed for Cid {
    const OFF: usize = MM_CID_OFF;

    fn get(self) -> u32 {
        self.0
    }
}

impl Index for Cid {}

/// NUMA node id from `area.node_id`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(u32);

impl NodeId {
    /// Wrap a kernel node id.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The numeric node id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// This thread's registered `struct rseq`. `Copy`. `NonNull` so it is not `Send`.
///
/// A self-registered [`Thread`] is valid until its thread exits. Call
/// [`crate::Rseq::bind`] again in a child after `fork`. The process
/// registration stays valid; this pointer does not.
#[derive(Clone, Copy, Debug)]
pub struct Thread {
    area: NonNull<Area>,
    node: bool,
    cid: bool,
    slice: bool,
}

impl Thread {
    pub(crate) const fn new(area: NonNull<Area>, node: bool, cid: bool, slice: bool) -> Self {
        Self {
            area,
            node,
            cid,
            slice,
        }
    }

    /// Kernel `cpu_id`. `None` if unregistered or a sentinel.
    ///
    /// librseq `rseq_current_cpu_raw`.
    #[must_use]
    pub fn cpu_id(&self) -> Option<CpuId> {
        // SAFETY: `area` is a registered rseq TLS; the kernel writes `cpu_id`.
        CpuId::new(unsafe { ptr::addr_of!((*self.area.as_ptr()).cpu_id).read_volatile() })
    }

    /// Speculative `cpu_id_start`. Read only. Side effects still require
    /// [`Self::cpu_id`] or [`Self::cid`] to confirm the index. The CS never
    /// uses this field as the key.
    ///
    /// librseq `rseq_cpu_start`.
    #[must_use]
    pub fn cpu_id_start(&self) -> Option<CpuId> {
        // SAFETY: `area` is a registered rseq TLS; the kernel writes `cpu_id_start`.
        CpuId::new(unsafe { ptr::addr_of!((*self.area.as_ptr()).cpu_id_start).read_volatile() })
    }

    /// Current CPU: [`Self::cpu_id`], then `sched_getcpu`.
    ///
    /// librseq `rseq_current_cpu`.
    #[must_use]
    pub fn cpu(&self) -> CpuId {
        self.cpu_id().unwrap_or_else(fallback_cpu)
    }

    /// Kernel `node_id`. `None` if this kernel does not populate it.
    ///
    /// librseq `rseq_current_node_id` / `rseq_node_id_available`.
    #[must_use]
    pub fn node_id(&self) -> Option<NodeId> {
        if !self.node {
            return None;
        }
        // SAFETY: `area` is 32 bytes; `getauxval` said `node_id` is live.
        Some(NodeId::new(unsafe {
            ptr::addr_of!((*self.area.as_ptr()).node_id).read_volatile()
        }))
    }

    /// Current NUMA node: [`Self::node_id`], then `getcpu`.
    #[must_use]
    pub fn node(&self) -> NodeId {
        self.node_id().unwrap_or_else(fallback_node)
    }

    /// Kernel `mm_cid`. `None` if this kernel does not populate it.
    ///
    /// librseq `rseq_current_mm_cid` / `rseq_mm_cid_available`.
    #[must_use]
    pub fn cid(&self) -> Option<Cid> {
        if !self.cid {
            return None;
        }
        // SAFETY: `area` is 32 bytes; `getauxval` said `mm_cid` is live.
        Some(Cid::new(unsafe {
            ptr::addr_of!((*self.area.as_ptr()).mm_cid).read_volatile()
        }))
    }

    /// Kernel `slice_ctrl`. `None` until auxv covers the field.
    ///
    /// librseq `rseq_slice_ctrl_available`.
    #[must_use]
    pub fn slice_ctrl(&self) -> Option<u32> {
        if !self.slice {
            return None;
        }
        // SAFETY: `area` is 32 bytes; `getauxval` said `slice_ctrl` is live.
        Some(unsafe { ptr::addr_of!((*self.area.as_ptr()).slice_ctrl).read_volatile() })
    }

    /// Clear `rseq_cs` before reclaiming CS descriptors or JIT code.
    ///
    /// librseq `rseq_prepare_unload` / `rseq_clear_rseq_cs`.
    pub fn prepare_unload(self) {
        // SAFETY: user-space may write `rseq_cs` only; this thread owns the area.
        unsafe {
            ptr::addr_of_mut!((*self.area.as_ptr()).rseq_cs).write_volatile(0);
        }
    }

    /// Compare `word` to `expect` and store `new`.
    ///
    /// This is librseq `cmpeqv_storev` / `rseq_load_cbne_store`, not
    /// [`core::sync::atomic::AtomicUsize::compare_exchange`].
    /// `Abort` is an index mismatch, not a spurious CAS failure.
    ///
    /// One attempt. Kernel preemption restarts inside the CS. Index mismatch
    /// is [`Error::Abort`] — re-read [`Self::cpu_id`] or [`Self::cid`] and
    /// pick a new word.
    ///
    /// # Errors
    ///
    /// [`Error::Miss`] when the word is not `expect`. [`Error::Abort`] when
    /// this thread's index is not `word.key`, or rseq is unavailable on this
    /// target.
    #[inline]
    pub fn compare_exchange<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
    ) -> Result<usize, Error> {
        // SAFETY: `self` is bound; `word` is a live AtomicUsize for `word.key`.
        // The CS is a Relaxed atomic RMW; rseq is atomicity vs same-index threads.
        match Self::cas::<K>(self.area, word, expect, new) {
            Attempt::Ok(old) => Ok(old),
            Attempt::Miss(current) => Err(Error::Miss(current)),
            Attempt::Abort => Err(Error::Abort),
        }
    }

    /// Compare `word` to `expect` and `other` to `other_expect`, then store `new`.
    ///
    /// librseq `cmpeqv_cmpeqv_storev` / `rseq_load_cbne_load_cbne_store`.
    /// Scratch-free. One committing store to `word`.
    ///
    /// `other.key` must equal `word.key` or this returns [`Error::Abort`]
    /// without entering the CS.
    ///
    /// [`Error::Miss`] carries the seen value of the compare that failed
    /// (`word` first, else `other`).
    ///
    /// # Errors
    ///
    /// [`Error::Miss`] when either compare fails. [`Error::Abort`] when the
    /// keys differ, this thread's index is not `word.key`, or rseq is
    /// unavailable on this target.
    #[inline]
    pub fn compare_exchange_if<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        other: Word<'_, K>,
        other_expect: usize,
    ) -> Result<usize, Error> {
        if other.key() != word.key() {
            return Err(Error::Abort);
        }
        // SAFETY: `self` is bound; both words are live AtomicUsizes for `word.key`.
        match Self::cas_if::<K>(self.area, word, expect, new, other, other_expect) {
            Attempt::Ok(old) => Ok(old),
            Attempt::Miss(current) => Err(Error::Miss(current)),
            Attempt::Abort => Err(Error::Abort),
        }
    }

    /// Add `count` to `word`. Returns the previous value.
    ///
    /// librseq `addv` / `rseq_load_add_store`.
    ///
    /// One attempt. Kernel preemption restarts inside the CS. Index mismatch
    /// is [`Error::Abort`] — re-read [`Self::cpu_id`] or [`Self::cid`] and
    /// pick a new word.
    ///
    /// # Errors
    ///
    /// [`Error::Abort`] when this thread's index is not `word.key`, or rseq
    /// is unavailable on this target.
    #[inline]
    pub fn fetch_add<K: Index>(&self, word: Word<'_, K>, count: usize) -> Result<usize, Error> {
        // SAFETY: `self` is bound; `word` is a live AtomicUsize for `word.key`.
        // The CS is a Relaxed atomic RMW; rseq is atomicity vs same-index threads.
        match Self::add::<K>(self.area, word, count) {
            Attempt::Ok(prev) => Ok(prev),
            Attempt::Miss(_) | Attempt::Abort => Err(Error::Abort),
        }
    }

    /// Increment the word at the address loaded from `*ptr + offset`.
    ///
    /// librseq `offset_deref_addv` / `rseq_load_add_load_load_add_store`.
    /// `offset` is a byte offset. A wrong offset is the caller's bug, same as C.
    ///
    /// Returns the previous value of the chased word.
    ///
    /// # Errors
    ///
    /// [`Error::Abort`] when this thread's index is not `ptr.key`, or rseq
    /// is unavailable on this target.
    #[inline]
    pub fn fetch_add_at<K: Index>(
        &self,
        ptr: Word<'_, K>,
        offset: isize,
        count: usize,
    ) -> Result<usize, Error> {
        // SAFETY: `self` is bound; `ptr` is a live AtomicUsize for `ptr.key`.
        // The caller guarantees `*ptr + offset` is a live pointer to a word.
        match Self::add_at::<K>(self.area, ptr, offset, count) {
            Attempt::Ok(prev) => Ok(prev),
            Attempt::Miss(_) | Attempt::Abort => Err(Error::Abort),
        }
    }

    /// If `*word != expect_not`, store the old `*word` into `out` and set
    /// `*word = *(*word + offset)`.
    ///
    /// librseq `cmpnev_storeoffp_load` / `rseq_load_cbeq_store_add_load_store`.
    /// `offset` is a byte offset from the loaded pointer. A wrong offset is
    /// the caller's bug, same as C.
    ///
    /// `out.key` must equal `word.key` or this returns [`Error::Abort`]
    /// without entering the CS. Returns the old `*word`.
    ///
    /// # Errors
    ///
    /// [`Error::Miss`] when `*word == expect_not`. [`Error::Abort`] when the
    /// keys differ, this thread's index is not `word.key`, or rseq is
    /// unavailable on this target.
    #[inline]
    pub fn load_if_ne<K: Index>(
        &self,
        word: Word<'_, K>,
        expect_not: usize,
        offset: isize,
        out: Word<'_, K>,
    ) -> Result<usize, Error> {
        if out.key() != word.key() {
            return Err(Error::Abort);
        }
        // SAFETY: `self` is bound; both words are live AtomicUsizes for `word.key`.
        // The caller guarantees `*word + offset` is a live pointer.
        match Self::load_ne::<K>(self.area, word, expect_not, offset, out) {
            Attempt::Ok(old) => Ok(old),
            Attempt::Miss(current) => Err(Error::Miss(current)),
            Attempt::Abort => Err(Error::Abort),
        }
    }

    /// Compare `word` to `expect`, store `side_new` into `side`, then store `new`.
    ///
    /// This is librseq `cmpeqv_trystorev_storev` / `rseq_load_cbne_store_store`
    /// (Relaxed). The store to `side` is scratch: an abort after it restarts
    /// and may write `side` again. The store to `word` is the commit.
    ///
    /// `side.key` must equal `word.key` or this returns [`Error::Abort`]
    /// without entering the CS.
    ///
    /// One attempt. Kernel preemption restarts inside the CS. Index mismatch
    /// is [`Error::Abort`] — re-read [`Self::cpu_id`] or [`Self::cid`] and
    /// pick a new word.
    ///
    /// ```compile_fail
    /// use rseq_rs::{Cid, CpuId, Thread, Word};
    /// fn mix(
    ///     t: &Thread,
    ///     cpu: Word<'_, CpuId>,
    ///     cid: Word<'_, Cid>,
    /// ) {
    ///     let _ = t.store_if(cpu, 0, 1, cid, 2);
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Miss`] when the word is not `expect` (`side` is untouched).
    /// [`Error::Abort`] when the keys differ, this thread's index is not
    /// `word.key`, or rseq is unavailable on this target.
    #[inline]
    pub fn store_if<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        side: Word<'_, K>,
        side_new: usize,
    ) -> Result<usize, Error> {
        self.store_if_mo::<K, false>(word, expect, new, side, side_new)
    }

    /// [`Self::store_if`] with a Release committing store.
    ///
    /// librseq `RSEQ_MO_RELEASE` on `rseq_load_cbne_store_store`.
    ///
    /// # Errors
    ///
    /// Same as [`Self::store_if`].
    #[inline]
    pub fn store_if_release<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        side: Word<'_, K>,
        side_new: usize,
    ) -> Result<usize, Error> {
        self.store_if_mo::<K, true>(word, expect, new, side, side_new)
    }

    /// Compare `word` to `expect`, memcpy `dst ← src` (scratch), then store `new`.
    ///
    /// librseq `cmpeqv_trymemcpy_storev` / `rseq_load_cbne_memcpy_store`
    /// (Relaxed). Length mismatch or a copy longer than [`COPY_MAX`] is
    /// [`Error::Abort`] in Rust before the CS.
    ///
    /// # Errors
    ///
    /// [`Error::Miss`] when the word is not `expect` (`dst` is untouched).
    /// [`Error::Abort`] on length/`COPY_MAX`, index mismatch, or when rseq
    /// is unavailable on this target.
    #[inline]
    pub fn store_if_copy<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        dst: &mut [u8],
        src: &[u8],
    ) -> Result<usize, Error> {
        self.store_if_copy_mo::<K, false>(word, expect, new, dst, src)
    }

    /// [`Self::store_if_copy`] with a Release committing store.
    ///
    /// librseq `RSEQ_MO_RELEASE` on `rseq_load_cbne_memcpy_store`.
    ///
    /// # Errors
    ///
    /// Same as [`Self::store_if_copy`].
    #[inline]
    pub fn store_if_copy_release<K: Index>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        dst: &mut [u8],
        src: &[u8],
    ) -> Result<usize, Error> {
        self.store_if_copy_mo::<K, true>(word, expect, new, dst, src)
    }

    #[inline]
    fn store_if_mo<K: Index, const RELEASE: bool>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        side: Word<'_, K>,
        side_new: usize,
    ) -> Result<usize, Error> {
        if side.key() != word.key() {
            return Err(Error::Abort);
        }
        // SAFETY: `self` is bound; both words are live AtomicUsizes for
        // `word.key`. The CS is an atomic RMW; rseq is atomicity vs
        // same-index threads.
        match Self::try_store::<K, RELEASE>(self.area, word, expect, new, side, side_new) {
            Attempt::Ok(old) => Ok(old),
            Attempt::Miss(current) => Err(Error::Miss(current)),
            Attempt::Abort => Err(Error::Abort),
        }
    }

    #[inline]
    fn store_if_copy_mo<K: Index, const RELEASE: bool>(
        &self,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        dst: &mut [u8],
        src: &[u8],
    ) -> Result<usize, Error> {
        if dst.len() != src.len() || src.len() > COPY_MAX {
            return Err(Error::Abort);
        }
        // SAFETY: `self` is bound; `word` is live; `dst`/`src` are the same
        // length and no longer than `COPY_MAX`.
        match Self::try_copy::<K, RELEASE>(self.area, word, expect, new, dst, src) {
            Attempt::Ok(old) => Ok(old),
            Attempt::Miss(current) => Err(Error::Miss(current)),
            Attempt::Abort => Err(Error::Abort),
        }
    }

    fn cas<K: Index>(area: NonNull<Area>, word: Word<'_, K>, expect: usize, new: usize) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        // SAFETY: `area` is this thread's rseq TLS; `word` is a live AtomicUsize.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::compare_exchange::<CPU_ID_OFF>(area, ptr, id, expect, new)
            } else {
                cs::compare_exchange::<MM_CID_OFF>(area, ptr, id, expect, new)
            }
        }
    }

    fn cas_if<K: Index>(
        area: NonNull<Area>,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        other: Word<'_, K>,
        other_expect: usize,
    ) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        let other_ptr = other.as_ptr();
        // SAFETY: `area` is this thread's rseq TLS; both words are live AtomicUsizes.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::compare_exchange_if::<CPU_ID_OFF>(
                    area,
                    ptr,
                    id,
                    expect,
                    new,
                    other_ptr,
                    other_expect,
                )
            } else {
                cs::compare_exchange_if::<MM_CID_OFF>(
                    area,
                    ptr,
                    id,
                    expect,
                    new,
                    other_ptr,
                    other_expect,
                )
            }
        }
    }

    fn add<K: Index>(area: NonNull<Area>, word: Word<'_, K>, count: usize) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        // SAFETY: `area` is this thread's rseq TLS; `word` is a live AtomicUsize.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::fetch_add::<CPU_ID_OFF>(area, ptr, id, count)
            } else {
                cs::fetch_add::<MM_CID_OFF>(area, ptr, id, count)
            }
        }
    }

    fn add_at<K: Index>(
        area: NonNull<Area>,
        ptr: Word<'_, K>,
        offset: isize,
        count: usize,
    ) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let word = ptr.as_ptr();
        let id = ptr.key().get();
        // SAFETY: `area` is this thread's rseq TLS; `ptr` is a live AtomicUsize.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::fetch_add_at::<CPU_ID_OFF>(area, word, id, offset, count)
            } else {
                cs::fetch_add_at::<MM_CID_OFF>(area, word, id, offset, count)
            }
        }
    }

    fn load_ne<K: Index>(
        area: NonNull<Area>,
        word: Word<'_, K>,
        expect_not: usize,
        offset: isize,
        out: Word<'_, K>,
    ) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        let out_ptr = out.as_ptr();
        // SAFETY: `area` is this thread's rseq TLS; both words are live AtomicUsizes.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::load_if_ne::<CPU_ID_OFF>(area, ptr, id, expect_not, offset, out_ptr)
            } else {
                cs::load_if_ne::<MM_CID_OFF>(area, ptr, id, expect_not, offset, out_ptr)
            }
        }
    }

    fn try_store<K: Index, const RELEASE: bool>(
        area: NonNull<Area>,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        side: Word<'_, K>,
        side_new: usize,
    ) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        let side_ptr = side.as_ptr();
        // SAFETY: `area` is this thread's rseq TLS; both words are live AtomicUsizes.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::store_if::<CPU_ID_OFF, RELEASE>(area, ptr, id, expect, new, side_ptr, side_new)
            } else {
                cs::store_if::<MM_CID_OFF, RELEASE>(area, ptr, id, expect, new, side_ptr, side_new)
            }
        }
    }

    fn try_copy<K: Index, const RELEASE: bool>(
        area: NonNull<Area>,
        word: Word<'_, K>,
        expect: usize,
        new: usize,
        dst: &mut [u8],
        src: &[u8],
    ) -> Attempt {
        const {
            assert!(K::OFF == CPU_ID_OFF || K::OFF == MM_CID_OFF);
        }
        let ptr = word.as_ptr();
        let id = word.key().get();
        let copy = Memcpy {
            dst: dst.as_mut_ptr(),
            src: src.as_ptr(),
            len: src.len(),
        };
        // SAFETY: `area` is this thread's rseq TLS; `word` is live; slices match.
        unsafe {
            if K::OFF == CPU_ID_OFF {
                cs::store_if_copy::<CPU_ID_OFF, RELEASE>(area, ptr, id, expect, new, copy)
            } else {
                cs::store_if_copy::<MM_CID_OFF, RELEASE>(area, ptr, id, expect, new, copy)
            }
        }
    }
}

fn fallback_cpu() -> CpuId {
    // SAFETY: `sched_getcpu` has no memory operands.
    let n = unsafe { libc::sched_getcpu() };
    u32::try_from(n)
        .ok()
        .and_then(CpuId::new)
        .unwrap_or(CpuId(0))
}

fn fallback_node() -> NodeId {
    let mut cpu = 0u32;
    let mut node = 0u32;
    // SAFETY: `getcpu` writes the two out-params; third arg is unused.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_getcpu,
            ptr::from_mut(&mut cpu),
            ptr::from_mut(&mut node),
            ptr::null::<u8>(),
        )
    };
    if rc == 0 {
        NodeId::new(node)
    } else {
        NodeId::new(0)
    }
}
