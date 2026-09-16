use core::fmt::{self, Display};
use core::ptr::{self, NonNull};

use crate::abi::{Area, CPU_ID_OFF, CPU_REG_FAILED, CPU_UNINIT, MM_CID_OFF};
use crate::attempt::Attempt;
use crate::cs;
use crate::words::Word;

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

/// This thread's registered `struct rseq`. `Copy`. `NonNull` so it is not `Send`.
///
/// A self-registered [`Thread`] is valid until its thread exits. Call
/// [`crate::Rseq::bind`] again in a child after `fork`. The process
/// registration stays valid; this pointer does not.
#[derive(Clone, Copy, Debug)]
pub struct Thread {
    area: NonNull<Area>,
    cid: bool,
}

impl Thread {
    pub(crate) const fn new(area: NonNull<Area>, cid: bool) -> Self {
        Self { area, cid }
    }

    /// Kernel `cpu_id`. `None` if unregistered or a sentinel.
    #[must_use]
    pub fn cpu_id(&self) -> Option<CpuId> {
        // SAFETY: `area` is a registered rseq TLS; the kernel writes `cpu_id`.
        CpuId::new(unsafe { ptr::addr_of!((*self.area.as_ptr()).cpu_id).read_volatile() })
    }

    /// Kernel `mm_cid`. `None` if this kernel does not populate it.
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

    /// Compare `word` to `expect` and store `new`.
    ///
    /// This is librseq `cmpeqv_storev`, not [`core::sync::atomic::AtomicUsize::compare_exchange`].
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

    /// Add `count` to `word`. Returns the previous value.
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

    /// Compare `word` to `expect`, store `side_new` into `side`, then store `new`.
    ///
    /// This is librseq `cmpeqv_trystorev_storev`. The store to `side` is
    /// scratch: an abort after it restarts and may write `side` again. The
    /// store to `word` is the commit.
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
        if side.key() != word.key() {
            return Err(Error::Abort);
        }
        // SAFETY: `self` is bound; both words are live AtomicUsizes for
        // `word.key`. The CS is a Relaxed atomic RMW; rseq is atomicity
        // vs same-index threads.
        match Self::try_store::<K>(self.area, word, expect, new, side, side_new) {
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

    fn try_store<K: Index>(
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
                cs::store_if::<CPU_ID_OFF>(area, ptr, id, expect, new, side_ptr, side_new)
            } else {
                cs::store_if::<MM_CID_OFF>(area, ptr, id, expect, new, side_ptr, side_new)
            }
        }
    }
}
