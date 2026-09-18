//! `mm_cid` word ops.

#[path = "common/skip.rs"]
mod skip;

use core::sync::atomic::{AtomicUsize, Ordering};

use rseq_rs::{Error, Rseq, Word};

use skip::skip;

#[test]
fn cid_ops() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let Some(cid) = thread.cid() else {
        skip("mm_cid not populated");
        return;
    };
    assert!(cid.get() < rseq.cpus());
    let words = rseq.words().expect("words");
    let w = words.get(cid).expect("word");
    assert_eq!(thread.compare_exchange(w, 0, 7), Ok(0));
    assert_eq!(thread.fetch_add(w, 1), Ok(7));
    let side = AtomicUsize::new(0);
    let s = Word::new(&side, cid);
    assert_eq!(thread.store_if(w, 8, 9, s, 3), Ok(8));
    assert_eq!(side.load(Ordering::Relaxed), 3);
    let other_word = AtomicUsize::new(1);
    let o = Word::new(&other_word, cid);
    assert_eq!(thread.compare_exchange_if(w, 9, 10, o, 1), Ok(9));

    let other = std::thread::scope(|scope| {
        scope
            .spawn(|| rseq.bind().expect("child bind").cid().expect("child cid"))
            .join()
            .expect("child")
    });
    if other != cid {
        let w = words.get(other).expect("other cid");
        assert_eq!(thread.compare_exchange(w, 0, 1), Err(Error::Abort));
    }
}

#[test]
fn cid_spawned_thread() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    if rseq.bind().and_then(|t| t.cid()).is_none() {
        skip("mm_cid not populated");
        return;
    }
    let join = std::thread::spawn(move || {
        let thread = rseq.bind().expect("child bind");
        let cid = thread.cid().expect("child cid");
        assert!(cid.get() < rseq.cpus());
    });
    join.join().expect("child");
}
