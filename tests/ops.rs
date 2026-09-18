//! Word ops on the current CPU.

#[path = "common/pin.rs"]
mod pin;
#[path = "common/skip.rs"]
mod skip;

use core::sync::atomic::{AtomicUsize, Ordering};

use rseq_rs::{CpuId, Error, Rseq, Word};

use pin::pin;
use skip::skip;

#[test]
fn compare_exchange_and_add() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");
    let words = rseq.words().expect("words");
    let w = words.get(cpu).expect("word");
    assert_eq!(thread.compare_exchange(w, 0, 7), Ok(0));
    assert_eq!(thread.compare_exchange(w, 0, 1), Err(Error::Miss(7)));
    assert_eq!(thread.fetch_add(w, 1), Ok(7));
    assert_eq!(thread.compare_exchange(w, 8, 8), Ok(8));
    let side = AtomicUsize::new(0);
    let s = Word::new(&side, cpu);
    assert_eq!(thread.store_if(w, 8, 9, s, 3), Ok(8));
    assert_eq!(side.load(Ordering::Relaxed), 3);
    side.store(4, Ordering::Relaxed);
    assert_eq!(thread.store_if(w, 0, 1, s, 5), Err(Error::Miss(9)));
    assert_eq!(side.load(Ordering::Relaxed), 4);
}

#[test]
fn isolated_cpus() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let words = rseq.words().expect("words");
    let thread = rseq.bind().expect("bind");
    let a = thread.cpu_id().expect("cpu");
    let b = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != a)
        .expect("other cpu");
    if !pin(a.get()) {
        skip(&format!("pin {a:?}"));
        return;
    }
    let t = rseq.bind().expect("bind a");
    let wa = words.get(t.cpu_id().expect("cpu a")).expect("word a");
    assert_eq!(t.compare_exchange(wa, 0, 11), Ok(0));
    if !pin(b.get()) {
        skip(&format!("pin {b:?}"));
        return;
    }
    let t = rseq.bind().expect("bind b");
    let wb = words.get(t.cpu_id().expect("cpu b")).expect("word b");
    assert_eq!(t.compare_exchange(wb, 0, 22), Ok(0));
    if !pin(a.get()) {
        skip(&format!("re-pin {a:?}"));
        return;
    }
    let t = rseq.bind().expect("rebind a");
    let wa = words.get(t.cpu_id().expect("cpu a")).expect("word a");
    assert_eq!(t.compare_exchange(wa, 11, 11), Ok(11));
    if !pin(b.get()) {
        skip(&format!("re-pin {b:?}"));
        return;
    }
    let t = rseq.bind().expect("rebind b");
    let wb = words.get(t.cpu_id().expect("cpu b")).expect("word b");
    assert_eq!(t.compare_exchange(wb, 22, 22), Ok(22));
}

#[test]
fn wrong_cpu_aborts() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let words = rseq.words().expect("words");
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    if !pin(here.get()) {
        skip(&format!("pin {here:?}"));
        return;
    }
    let thread = rseq.bind().expect("bind pinned");
    let here = thread.cpu_id().expect("cpu");
    let other = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let w = words.get(other).expect("other word");
    assert_eq!(thread.compare_exchange(w, 0, 1), Err(Error::Abort));
    assert_eq!(thread.fetch_add(w, 1), Err(Error::Abort));
    let side = AtomicUsize::new(0);
    let s = Word::new(&side, other);
    assert_eq!(thread.store_if(w, 0, 1, s, 2), Err(Error::Abort));
    assert_eq!(side.load(Ordering::Relaxed), 0);
}

#[test]
fn store_if_side_cpu_mismatch() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    let other = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let words = rseq.words().expect("words");
    let w = words.get(here).expect("word");
    let side = AtomicUsize::new(0);
    let s = Word::new(&side, other);
    assert_eq!(thread.store_if(w, 0, 1, s, 2), Err(Error::Abort));
    assert_eq!(side.load(Ordering::Relaxed), 0);
}

#[test]
fn compare_exchange_if_and_miss() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");
    let words = rseq.words().expect("words");
    let w = words.get(cpu).expect("word");
    let other = AtomicUsize::new(4);
    let o = Word::new(&other, cpu);
    assert_eq!(thread.compare_exchange(w, 0, 1), Ok(0));
    assert_eq!(thread.compare_exchange_if(w, 1, 2, o, 4), Ok(1));
    assert_eq!(
        thread.compare_exchange_if(w, 0, 9, o, 4),
        Err(Error::Miss(2))
    );
    assert_eq!(
        thread.compare_exchange_if(w, 2, 9, o, 0),
        Err(Error::Miss(4))
    );
    assert_eq!(thread.compare_exchange(w, 2, 2), Ok(2));
    assert_eq!(other.load(Ordering::Relaxed), 4);
}

#[test]
fn compare_exchange_if_alias() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");
    let words = rseq.words().expect("words");
    let w = words.get(cpu).expect("word");
    assert_eq!(thread.compare_exchange(w, 0, 5), Ok(0));
    // `other` aliases `word`: the CS loads the same address twice.
    assert_eq!(thread.compare_exchange_if(w, 5, 6, w, 5), Ok(5));
    assert_eq!(thread.compare_exchange(w, 6, 6), Ok(6));
    assert_eq!(
        thread.compare_exchange_if(w, 6, 7, w, 0),
        Err(Error::Miss(6))
    );
}

#[test]
fn compare_exchange_if_key_mismatch() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    let other_cpu = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let words = rseq.words().expect("words");
    let w = words.get(here).expect("word");
    let other = AtomicUsize::new(0);
    let o = Word::new(&other, other_cpu);
    assert_eq!(thread.compare_exchange_if(w, 0, 1, o, 0), Err(Error::Abort));
    assert_eq!(other.load(Ordering::Relaxed), 0);
    assert_eq!(thread.compare_exchange(w, 0, 0), Ok(0));
}

#[test]
fn compare_exchange_if_wrong_cpu() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let words = rseq.words().expect("words");
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    if !pin(here.get()) {
        skip(&format!("pin {here:?}"));
        return;
    }
    let thread = rseq.bind().expect("bind pinned");
    let here = thread.cpu_id().expect("cpu");
    let other_cpu = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let w = words.get(other_cpu).expect("other word");
    let side = AtomicUsize::new(0);
    let o = Word::new(&side, other_cpu);
    assert_eq!(thread.compare_exchange_if(w, 0, 1, o, 0), Err(Error::Abort));
    assert_eq!(side.load(Ordering::Relaxed), 0);
}

#[repr(C)]
struct Node {
    tag: usize,
    next: AtomicUsize,
}

#[repr(C)]
struct Owner {
    tag: usize,
    counter: AtomicUsize,
}

#[test]
fn load_if_ne_pop_and_miss() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");
    let tail = Node {
        tag: 0,
        next: AtomicUsize::new(0),
    };
    let node = Node {
        tag: 0,
        next: AtomicUsize::new(core::ptr::from_ref(&tail) as usize),
    };
    let head = AtomicUsize::new(core::ptr::from_ref(&node) as usize);
    let h = Word::new(&head, cpu);
    let off = core::mem::offset_of!(Node, next) as isize;
    // SAFETY: `head` points at `node`; `node.next` is a live usize.
    let old = unsafe { thread.load_if_ne(h, 0, off) };
    assert_eq!(old, Ok(core::ptr::from_ref(&node) as usize));
    assert_eq!(
        head.load(Ordering::Relaxed),
        core::ptr::from_ref(&tail) as usize
    );
    // SAFETY: `head` now points at `tail`; `tail.next` is a live usize.
    let old = unsafe { thread.load_if_ne(h, 0, off) };
    assert_eq!(old, Ok(core::ptr::from_ref(&tail) as usize));
    assert_eq!(head.load(Ordering::Relaxed), 0);
    // SAFETY: `*head == 0 == expect_not`, so the CS does not dereference.
    assert_eq!(unsafe { thread.load_if_ne(h, 0, off) }, Err(Error::Miss(0)));
    assert_eq!(head.load(Ordering::Relaxed), 0);
}

#[test]
fn fetch_add_at_through_field() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");
    let target = AtomicUsize::new(10);
    let owner = Owner {
        tag: 0,
        counter: AtomicUsize::new(core::ptr::from_ref(&target) as usize),
    };
    let base = AtomicUsize::new(core::ptr::from_ref(&owner) as usize);
    let p = Word::new(&base, cpu);
    let off = core::mem::offset_of!(Owner, counter) as isize;
    // SAFETY: `*base + off` is `owner.counter`, which holds `&target`.
    assert_eq!(unsafe { thread.fetch_add_at(p, off, 3) }, Ok(10));
    assert_eq!(target.load(Ordering::Relaxed), 13);
    assert_eq!(
        base.load(Ordering::Relaxed),
        core::ptr::from_ref(&owner) as usize
    );
}

#[test]
fn pointer_chase_wrong_cpu() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        skip("need two CPUs");
        return;
    }
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    if !pin(here.get()) {
        skip(&format!("pin {here:?}"));
        return;
    }
    let thread = rseq.bind().expect("bind pinned");
    let here = thread.cpu_id().expect("cpu");
    let other = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let tail = Node {
        tag: 0,
        next: AtomicUsize::new(0),
    };
    let node = Node {
        tag: 0,
        next: AtomicUsize::new(core::ptr::from_ref(&tail) as usize),
    };
    let head = AtomicUsize::new(core::ptr::from_ref(&node) as usize);
    let h = Word::new(&head, other);
    let pop_off = core::mem::offset_of!(Node, next) as isize;
    // SAFETY: index mismatch aborts before any chase.
    let pop = unsafe { thread.load_if_ne(h, 0, pop_off) };
    assert_eq!(pop, Err(Error::Abort));
    assert_eq!(
        head.load(Ordering::Relaxed),
        core::ptr::from_ref(&node) as usize
    );
    let target = AtomicUsize::new(10);
    let owner = Owner {
        tag: 0,
        counter: AtomicUsize::new(core::ptr::from_ref(&target) as usize),
    };
    let base = AtomicUsize::new(core::ptr::from_ref(&owner) as usize);
    let p = Word::new(&base, other);
    let add_off = core::mem::offset_of!(Owner, counter) as isize;
    // SAFETY: index mismatch aborts before any chase.
    let add = unsafe { thread.fetch_add_at(p, add_off, 3) };
    assert_eq!(add, Err(Error::Abort));
    assert_eq!(target.load(Ordering::Relaxed), 10);
    assert_eq!(
        base.load(Ordering::Relaxed),
        core::ptr::from_ref(&owner) as usize
    );
}
