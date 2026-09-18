use core::fmt::{self, Debug};
use core::hash::{Hash, Hasher};
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;

use crate::region::Region;
use crate::thread::{Cid, CpuId, Index};

/// One word and the index it belongs to. librseq's `(v, cpu)` / `(v, mm_cid)`.
///
/// [`Words::get`] borrows the region. [`Word::new`] borrows the atomic.
/// `K` defaults to [`CpuId`] so `Word<'_>` stays the cpu word.
#[derive(Clone, Copy, Debug)]
pub struct Word<'a, K: Index = CpuId> {
    word: &'a AtomicUsize,
    key: K,
}

impl<'a, K: Index> Word<'a, K> {
    /// Caller-owned word paired with `key`.
    #[must_use]
    pub const fn new(word: &'a AtomicUsize, key: K) -> Self {
        Self { word, key }
    }

    /// The index this word was picked with.
    #[must_use]
    pub const fn key(self) -> K {
        self.key
    }

    /// The borrowed atomic. Same address the CS stores through.
    #[must_use]
    pub const fn atomic(self) -> &'a AtomicUsize {
        self.word
    }

    /// Raw pointer for the CS. Same address as the borrowed atomic.
    #[must_use]
    pub(crate) const fn as_ptr(self) -> *mut AtomicUsize {
        core::ptr::from_ref(self.word).cast_mut()
    }
}

impl Word<'_, CpuId> {
    /// The CPU this word was picked with.
    #[must_use]
    pub const fn cpu(self) -> CpuId {
        self.key
    }
}

impl Word<'_, Cid> {
    /// The concurrency id this word was picked with.
    #[must_use]
    pub const fn cid(self) -> Cid {
        self.key
    }
}

impl<K: Index> PartialEq for Word<'_, K> {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.word, other.word) && self.key == other.key
    }
}

impl<K: Index> Eq for Word<'_, K> {}

impl<K: Index + Hash> Hash for Word<'_, K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::ptr::from_ref(self.word).hash(state);
        self.key.hash(state);
    }
}

/// Optional mmap of one word per possible CPU.
///
/// Slot count is possible CPUs. `mm_cid` is always `<` allowed CPUs `<=`
/// possible CPUs, so [`crate::Rseq::words`] covers cids. A smaller
/// [`Words::new`] is valid if [`Self::get`] may be `None`.
pub struct Words {
    region: Region,
    cpus: u32,
}

// SAFETY: the mapping is process-private `AtomicUsize`s. `NonNull<u8>` is
// `!Send`/`!Sync`; the words are the atomics the CS and drain already share.
unsafe impl Send for Words {}
// SAFETY: same as `Send`: exclusive mmap of atomics shared only via Relaxed CS / drain.
unsafe impl Sync for Words {}

impl Debug for Words {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Words")
            .field("cpus", &self.cpus)
            .field("base", &self.region.base())
            .finish()
    }
}

impl Words {
    /// Allocate `cpus` words. `None` if `cpus` is zero or mmap fails.
    #[must_use]
    pub fn new(cpus: u32) -> Option<Self> {
        if cpus == 0 {
            return None;
        }
        Some(Self {
            region: Region::map(cpus)?,
            cpus,
        })
    }

    /// Use a caller-owned `'static` slice of words.
    ///
    /// `None` if the slice is empty or longer than `u32::MAX`.
    #[must_use]
    pub fn from_static(words: &'static [AtomicUsize]) -> Option<Self> {
        let cpus = u32::try_from(words.len()).ok()?;
        if cpus == 0 {
            return None;
        }
        Some(Self {
            region: Region::raw(NonNull::from(&words[0]).cast()),
            cpus,
        })
    }

    /// Slot count. May be indexed by [`CpuId`] or [`Cid`].
    #[must_use]
    pub const fn cpus(&self) -> u32 {
        self.cpus
    }

    /// Pointer plus `key`. Address math, not a critical section.
    #[must_use]
    pub fn get<K: Index>(&self, key: K) -> Option<Word<'_, K>> {
        let id = key.get();
        if id >= self.cpus {
            return None;
        }
        // SAFETY: `id` is in range; mmap of usizes is aligned.
        let ptr = unsafe { self.region.word(id) };
        // SAFETY: `ptr` is a live aligned AtomicUsize in this region.
        Some(Word::new(unsafe { ptr.as_ref() }, key))
    }
}
