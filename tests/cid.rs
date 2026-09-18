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
    let tail = Node {
        tag: 0,
        next: AtomicUsize::new(0),
    };
    let node = Node {
        tag: 0,
        next: AtomicUsize::new(core::ptr::from_ref(&tail) as usize),
    };
    let head = AtomicUsize::new(core::ptr::from_ref(&node) as usize);
    let h = Word::new(&head, cid);
    let pop_off = core::mem::offset_of!(Node, next) as isize;
    // SAFETY: `head` points at `node`; `node.next` is a live usize.
    let popped = unsafe { thread.load_if_ne(h, 0, pop_off) };
    assert_eq!(popped, Ok(core::ptr::from_ref(&node) as usize));
    let target = AtomicUsize::new(10);
    let owner = Owner {
        tag: 0,
        counter: AtomicUsize::new(core::ptr::from_ref(&target) as usize),
    };
    let base = AtomicUsize::new(core::ptr::from_ref(&owner) as usize);
    let p = Word::new(&base, cid);
    let add_off = core::mem::offset_of!(Owner, counter) as isize;
    // SAFETY: `*base + add_off` is `owner.counter`, which holds `&target`.
    let prev = unsafe { thread.fetch_add_at(p, add_off, 3) };
    assert_eq!(prev, Ok(10));

    // Keep the child live so its `mm_cid` is not recycled onto this thread.
    std::thread::scope(|scope| {
        let (ready, wait_ready) = std::sync::mpsc::channel();
        let (release, wait_release) = std::sync::mpsc::channel();
        scope.spawn(move || {
            let child = rseq.bind().expect("child bind");
            ready.send(child.cid().expect("child cid")).expect("ready");
            wait_release.recv().expect("release");
        });
        let other = wait_ready.recv().expect("child cid");
        if other != cid {
            let w = words.get(other).expect("other cid");
            assert_eq!(thread.compare_exchange(w, 0, 1), Err(Error::Abort));
        }
        release.send(()).expect("release");
    });
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
