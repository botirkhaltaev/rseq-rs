//! Word ops on the current CPU.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};

use rseq_rs::{COPY_MAX, CpuId, Error, Rseq, Word};

use common::pin;

#[test]
fn compare_exchange_and_add() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
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
        eprintln!("skip: rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        eprintln!("skip: need two CPUs");
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
        eprintln!("skip: pin {a:?}");
        return;
    }
    let t = rseq.bind().expect("bind a");
    let wa = words.get(t.cpu_id().expect("cpu a")).expect("word a");
    assert_eq!(t.compare_exchange(wa, 0, 11), Ok(0));
    if !pin(b.get()) {
        eprintln!("skip: pin {b:?}");
        return;
    }
    let t = rseq.bind().expect("bind b");
    let wb = words.get(t.cpu_id().expect("cpu b")).expect("word b");
    assert_eq!(t.compare_exchange(wb, 0, 22), Ok(0));
    if !pin(a.get()) {
        eprintln!("skip: re-pin {a:?}");
        return;
    }
    let t = rseq.bind().expect("rebind a");
    let wa = words.get(t.cpu_id().expect("cpu a")).expect("word a");
    assert_eq!(t.compare_exchange(wa, 11, 11), Ok(11));
    if !pin(b.get()) {
        eprintln!("skip: re-pin {b:?}");
        return;
    }
    let t = rseq.bind().expect("rebind b");
    let wb = words.get(t.cpu_id().expect("cpu b")).expect("word b");
    assert_eq!(t.compare_exchange(wb, 22, 22), Ok(22));
}

#[test]
fn wrong_cpu_aborts() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        eprintln!("skip: need two CPUs");
        return;
    }
    let words = rseq.words().expect("words");
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    if !pin(here.get()) {
        eprintln!("skip: pin {here:?}");
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
        eprintln!("skip: rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        eprintln!("skip: need two CPUs");
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
fn compare_exchange_if_and_copy() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
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

    let side = AtomicUsize::new(0);
    let s = Word::new(&side, cpu);
    assert_eq!(thread.store_if_release(w, 2, 3, s, 8), Ok(2));
    assert_eq!(side.load(Ordering::Relaxed), 8);

    let src = [1u8, 2, 3];
    let mut dst = [0u8; 3];
    assert_eq!(thread.store_if_copy(w, 3, 4, &mut dst, &src), Ok(3));
    assert_eq!(dst, src);
    let mut stay = [9u8; 3];
    assert_eq!(
        thread.store_if_copy(w, 0, 1, &mut stay, &src),
        Err(Error::Miss(4))
    );
    assert_eq!(stay, [9, 9, 9]);
    assert_eq!(thread.store_if_copy_release(w, 4, 5, &mut dst, &src), Ok(4));
    let too_long = [0u8; COPY_MAX + 1];
    let mut too_dst = [0u8; COPY_MAX + 1];
    assert_eq!(
        thread.store_if_copy(w, 5, 6, &mut too_dst, &too_long),
        Err(Error::Abort)
    );
}

#[test]
fn load_if_ne_and_fetch_add_at() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu");

    let next = AtomicUsize::new(0);
    let node = AtomicUsize::new(core::ptr::from_ref(&next) as usize);
    let head = AtomicUsize::new(core::ptr::from_ref(&node) as usize);
    let out = AtomicUsize::new(0);
    let h = Word::new(&head, cpu);
    let o = Word::new(&out, cpu);
    let old = thread.load_if_ne(h, 0, 0, o).expect("pop");
    assert_eq!(old, core::ptr::from_ref(&node) as usize);
    assert_eq!(out.load(Ordering::Relaxed), old);
    assert_eq!(
        head.load(Ordering::Relaxed),
        core::ptr::from_ref(&next) as usize
    );
    let now = head.load(Ordering::Relaxed);
    assert_eq!(thread.load_if_ne(h, now, 0, o), Err(Error::Miss(now)));

    let counter = AtomicUsize::new(10);
    let field = AtomicUsize::new(core::ptr::from_ref(&counter) as usize);
    let base = AtomicUsize::new(core::ptr::from_ref(&field) as usize);
    let p = Word::new(&base, cpu);
    assert_eq!(thread.fetch_add_at(p, 0, 3), Ok(10));
    assert_eq!(counter.load(Ordering::Relaxed), 13);
}

#[test]
fn compare_exchange_if_wrong_cpu() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
        return;
    };
    if rseq.cpus() < 2 {
        eprintln!("skip: need two CPUs");
        return;
    }
    let thread = rseq.bind().expect("bind");
    let here = thread.cpu_id().expect("cpu");
    if !pin(here.get()) {
        eprintln!("skip: pin {here:?}");
        return;
    }
    let thread = rseq.bind().expect("bind pinned");
    let here = thread.cpu_id().expect("cpu");
    let other = (0..rseq.cpus())
        .filter_map(CpuId::new)
        .find(|id| *id != here)
        .expect("other cpu");
    let words = rseq.words().expect("words");
    let w = words.get(other).expect("other word");
    let side = AtomicUsize::new(0);
    let s = Word::new(&side, other);
    assert_eq!(thread.compare_exchange_if(w, 0, 1, s, 0), Err(Error::Abort));
}
