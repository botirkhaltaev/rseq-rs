//! Word ops on the current CPU.

mod common;

use core::sync::atomic::{AtomicUsize, Ordering};

use rseq_rs::{CpuId, Error, Rseq, Word};

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
