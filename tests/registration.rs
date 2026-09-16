//! Self-registration when glibc left rseq off.
//!
//! Run with `GLIBC_TUNABLES=glibc.pthread.rseq=0`.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use rseq_rs::{Error, Rseq, Word};

fn self_register_requested() -> bool {
    std::env::var("GLIBC_TUNABLES").is_ok_and(|v| v.contains("rseq=0"))
}

#[test]
fn self_register_ops() {
    if !self_register_requested() {
        eprintln!("skip: set GLIBC_TUNABLES=glibc.pthread.rseq=0");
        return;
    }
    let rseq = Rseq::try_new().expect("SYS_rseq when glibc rseq is off");
    let thread = rseq.bind().expect("bind");
    let again = rseq.bind().expect("bind twice");
    assert_eq!(thread.cpu_id(), again.cpu_id());
    let cpu = thread.cpu_id().expect("cpu");
    let words = rseq.words().expect("words");
    let w = words.get(cpu).expect("word");
    assert_eq!(thread.compare_exchange(w, 0, 7), Ok(0));
    assert_eq!(thread.fetch_add(w, 1), Ok(7));
    let side = AtomicUsize::new(0);
    // SAFETY: `side` is a live aligned word; this test owns it for the ops.
    let s = unsafe { Word::from_raw(NonNull::from(&side), cpu) };
    assert_eq!(thread.store_if(w, 8, 9, s, 3), Ok(8));
    assert_eq!(side.load(Ordering::Relaxed), 3);
}

#[test]
fn self_register_spawned_thread() {
    if !self_register_requested() {
        eprintln!("skip: set GLIBC_TUNABLES=glibc.pthread.rseq=0");
        return;
    }
    let rseq = Rseq::try_new().expect("SYS_rseq when glibc rseq is off");
    let join = std::thread::spawn(move || {
        let thread = rseq.bind().expect("child bind");
        let cpu = thread.cpu_id().expect("child cpu");
        let words = rseq.words().expect("child words");
        let w = words.get(cpu).expect("child word");
        assert_eq!(thread.compare_exchange(w, 0, 11), Ok(0));
        assert_eq!(thread.compare_exchange(w, 0, 1), Err(Error::Miss(11)));
    });
    join.join().expect("child");
}
