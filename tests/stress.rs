//! Abort-heavy uniqueness. `cargo test -p rseq-rs -- --ignored`.

#[path = "common/pin.rs"]
mod pin;
#[path = "common/skip.rs"]
mod skip;

use std::os::raw::c_int;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use rseq_rs::{CpuId, Error, Index, Rseq, Thread, Word, Words};

use pin::pin;
use skip::skip;

extern "C" fn ignore_alrm(_sig: c_int) {}

fn start_alrm() {
    // SAFETY: install a no-op SIGALRM handler and a 200µs interval timer.
    // Both syscalls take pointers to stack locals / the handler function.
    unsafe {
        libc::signal(
            libc::SIGALRM,
            ignore_alrm as *const () as libc::sighandler_t,
        );
        let mut it = std::mem::zeroed::<libc::itimerval>();
        it.it_interval.tv_usec = 200;
        it.it_value.tv_usec = 200;
        libc::setitimer(libc::ITIMER_REAL, &raw const it, std::ptr::null_mut());
    }
}

fn stop_alrm() {
    // SAFETY: clear the interval timer; pointer is to a stack zeroed itimerval.
    unsafe {
        let it = std::mem::zeroed::<libc::itimerval>();
        libc::setitimer(libc::ITIMER_REAL, &raw const it, std::ptr::null_mut());
    }
}

fn sum_words(rseq: Rseq, words: &Words) -> usize {
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let mut sum = 0;
    for cpu in 0..ncpus {
        pin(cpu as u32);
        loop {
            let thread = rseq.bind().expect("bind");
            let id = thread.cpu_id().expect("cpu");
            let w = words.get(id).expect("word");
            match thread.fetch_add(w, 0) {
                Ok(v) => {
                    sum += v;
                    break;
                }
                Err(Error::Abort) => {}
                Err(Error::Miss(_)) => unreachable!("add is not a compare"),
            }
        }
    }
    sum
}

fn unique_add<K: Index + Send + Sync>(
    rseq: Rseq,
    next: impl Fn(&Thread) -> Option<K> + Send + Sync + Copy + 'static,
) {
    let words = Arc::new(rseq.words().expect("words"));
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let nthreads = 8.min(ncpus.saturating_mul(2).max(2));
    let added = Arc::new(AtomicUsize::new(0));
    start_alrm();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut joins = Vec::new();
    for _ in 0..nthreads {
        let words = Arc::clone(&words);
        let added = Arc::clone(&added);
        joins.push(thread::spawn(move || {
            let thread = rseq.bind().expect("bind");
            let mut i = 0usize;
            while Instant::now() < deadline {
                pin((i % ncpus) as u32);
                let Some(key) = next(&thread) else {
                    i += 1;
                    continue;
                };
                let Some(w) = words.get(key) else {
                    i += 1;
                    continue;
                };
                if thread.fetch_add(w, 1).is_ok() {
                    added.fetch_add(1, Ordering::Relaxed);
                }
                i += 1;
            }
        }));
    }
    for j in joins {
        j.join().expect("join");
    }
    stop_alrm();
    assert_eq!(sum_words(rseq, &words), added.load(Ordering::Relaxed));
}

fn unique_store_if(rseq: Rseq) {
    let words = Arc::new(rseq.words().expect("words"));
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let nthreads = 8.min(ncpus.saturating_mul(2).max(2));
    let won = Arc::new(AtomicUsize::new(0));
    let sides: Arc<Vec<AtomicUsize>> = Arc::new((0..ncpus).map(|_| AtomicUsize::new(0)).collect());
    start_alrm();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut joins = Vec::new();
    for _ in 0..nthreads {
        let words = Arc::clone(&words);
        let won = Arc::clone(&won);
        let sides = Arc::clone(&sides);
        joins.push(thread::spawn(move || {
            let thread = rseq.bind().expect("bind");
            let mut i = 0usize;
            let mut expect = 0usize;
            while Instant::now() < deadline {
                pin((i % ncpus) as u32);
                let Some(cpu) = thread.cpu_id() else {
                    i += 1;
                    continue;
                };
                let Some(w) = words.get(cpu) else {
                    i += 1;
                    continue;
                };
                let side = Word::new(&sides[cpu.get() as usize], cpu);
                match thread.store_if(w, expect, expect.wrapping_add(1), side, expect) {
                    Ok(_) => {
                        won.fetch_add(1, Ordering::Relaxed);
                        expect = expect.wrapping_add(1);
                    }
                    Err(Error::Miss(current)) => expect = current,
                    Err(Error::Abort) => {}
                }
                i += 1;
            }
        }));
    }
    for j in joins {
        j.join().expect("join");
    }
    stop_alrm();
    assert_eq!(sum_words(rseq, &words), won.load(Ordering::Relaxed));
}

fn unique_compare_exchange_if(rseq: Rseq) {
    let words = Arc::new(rseq.words().expect("words"));
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let nthreads = 8.min(ncpus.saturating_mul(2).max(2));
    let won = Arc::new(AtomicUsize::new(0));
    start_alrm();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut joins = Vec::new();
    for _ in 0..nthreads {
        let words = Arc::clone(&words);
        let won = Arc::clone(&won);
        joins.push(thread::spawn(move || {
            let thread = rseq.bind().expect("bind");
            let mut i = 0usize;
            let mut expect = 0usize;
            while Instant::now() < deadline {
                pin((i % ncpus) as u32);
                let Some(cpu) = thread.cpu_id() else {
                    i += 1;
                    continue;
                };
                let Some(w) = words.get(cpu) else {
                    i += 1;
                    continue;
                };
                // Alias `other` with `word` so Miss always carries the word value.
                match thread.compare_exchange_if(w, expect, expect.wrapping_add(1), w, expect) {
                    Ok(_) => {
                        won.fetch_add(1, Ordering::Relaxed);
                        expect = expect.wrapping_add(1);
                    }
                    Err(Error::Miss(current)) => expect = current,
                    Err(Error::Abort) => {}
                }
                i += 1;
            }
        }));
    }
    for j in joins {
        j.join().expect("join");
    }
    stop_alrm();
    assert_eq!(sum_words(rseq, &words), won.load(Ordering::Relaxed));
}

#[ignore = "affinity flap and SIGALRM; run with --ignored"]
#[test]
fn unique_add_under_migration_and_signals() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    unique_add(rseq, Thread::cpu_id);
    if rseq.bind().and_then(|t| t.cid()).is_some() {
        unique_add(rseq, Thread::cid);
    }
}

#[ignore = "affinity flap and SIGALRM; run with --ignored"]
#[test]
fn no_lost_cas_under_migration_and_signals() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let words = Arc::new(rseq.words().expect("words"));
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let nthreads = 8.min(ncpus.saturating_mul(2).max(2));
    let won = Arc::new(AtomicUsize::new(0));
    start_alrm();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut joins = Vec::new();
    for _ in 0..nthreads {
        let words = Arc::clone(&words);
        let won = Arc::clone(&won);
        joins.push(thread::spawn(move || {
            let thread = rseq.bind().expect("bind");
            let mut i = 0usize;
            let mut expect = 0usize;
            while Instant::now() < deadline {
                pin((i % ncpus) as u32);
                let Some(cpu) = thread.cpu_id() else {
                    i += 1;
                    continue;
                };
                let Some(w) = words.get(cpu) else {
                    i += 1;
                    continue;
                };
                match thread.compare_exchange(w, expect, expect.wrapping_add(1)) {
                    Ok(_) => {
                        won.fetch_add(1, Ordering::Relaxed);
                        expect = expect.wrapping_add(1);
                    }
                    Err(Error::Miss(current)) => expect = current,
                    Err(Error::Abort) => {}
                }
                i += 1;
            }
        }));
    }
    for j in joins {
        j.join().expect("join");
    }
    stop_alrm();
    assert_eq!(sum_words(rseq, &words), won.load(Ordering::Relaxed));
}

#[ignore = "affinity flap and SIGALRM; run with --ignored"]
#[test]
fn no_lost_store_if_under_migration_and_signals() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    unique_store_if(rseq);
}

#[ignore = "affinity flap and SIGALRM; run with --ignored"]
#[test]
fn no_lost_compare_exchange_if_under_migration_and_signals() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    unique_compare_exchange_if(rseq);
}

#[repr(C)]
struct PopNode {
    tag: AtomicUsize,
    next: AtomicUsize,
}

fn unique_pop(rseq: Rseq) {
    const N: usize = 32;
    let words = Arc::new(rseq.words().expect("words"));
    let ncpus = usize::try_from(rseq.cpus()).expect("cpus");
    let nthreads = 8.min(ncpus.saturating_mul(2).max(2));
    let off = core::mem::offset_of!(PopNode, next) as isize;
    let mut lists: Vec<Vec<PopNode>> = (0..ncpus)
        .map(|_| {
            (0..N)
                .map(|_| PopNode {
                    tag: AtomicUsize::new(0),
                    next: AtomicUsize::new(0),
                })
                .collect()
        })
        .collect();
    for list in &mut lists {
        for i in 0..N.saturating_sub(1) {
            let nxt = core::ptr::from_ref(&list[i + 1]) as usize;
            list[i].next.store(nxt, Ordering::Relaxed);
        }
    }
    let lists = Arc::new(lists);
    for cpu in 0..ncpus {
        let id = CpuId::new(cpu as u32).expect("cpu");
        let w = words.get(id).expect("word");
        let head = core::ptr::from_ref(&lists[cpu][0]) as usize;
        w.atomic().store(head, Ordering::Relaxed);
    }
    let won = Arc::new(AtomicUsize::new(0));
    start_alrm();
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut joins = Vec::new();
    for _ in 0..nthreads {
        let words = Arc::clone(&words);
        let won = Arc::clone(&won);
        joins.push(thread::spawn(move || {
            let thread = rseq.bind().expect("bind");
            let mut i = 0usize;
            while Instant::now() < deadline {
                pin((i % ncpus) as u32);
                let Some(cpu) = thread.cpu_id() else {
                    i += 1;
                    continue;
                };
                let Some(w) = words.get(cpu) else {
                    i += 1;
                    continue;
                };
                // SAFETY: heads point at live `PopNode`s in `lists` for the test.
                match unsafe { thread.load_if_ne(w, 0, off) } {
                    Ok(p) => {
                        // SAFETY: `p` is a node from this CPU's list.
                        let node = unsafe { &*(p as *const PopNode) };
                        loop {
                            let Some(now) = thread.cpu_id() else {
                                continue;
                            };
                            let tag = Word::new(&node.tag, now);
                            match thread.fetch_add(tag, 1) {
                                Ok(_) => break,
                                Err(Error::Abort) => {}
                                Err(Error::Miss(_)) => unreachable!("add is not a compare"),
                            }
                        }
                        won.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(Error::Miss(_)) | Err(Error::Abort) => {}
                }
                i += 1;
            }
        }));
    }
    for j in joins {
        j.join().expect("join");
    }
    stop_alrm();
    assert_eq!(won.load(Ordering::Relaxed), N.saturating_mul(ncpus));
    for list in lists.iter() {
        for node in list {
            assert_eq!(node.tag.load(Ordering::Relaxed), 1);
        }
    }
    for cpu in 0..ncpus {
        let id = CpuId::new(cpu as u32).expect("cpu");
        assert_eq!(
            words
                .get(id)
                .expect("word")
                .atomic()
                .load(Ordering::Relaxed),
            0
        );
    }
}

#[ignore = "affinity flap and SIGALRM; run with --ignored"]
#[test]
fn no_lost_pop_under_migration_and_signals() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    unique_pop(rseq);
}
