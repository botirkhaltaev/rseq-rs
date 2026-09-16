//! Region sizing and `Words::from_static`.

use core::sync::atomic::AtomicUsize;

use rseq_rs::{CpuId, Word, Words};

fn cpu(id: u32) -> CpuId {
    CpuId::new(id).expect("not the uninit sentinel")
}

#[test]
fn new_sizes() {
    assert!(Words::new(0).is_none());
    let words = Words::new(2).expect("map");
    assert_eq!(words.cpus(), 2);
    let w = words.get(cpu(0)).expect("cpu 0");
    assert_eq!(w.cpu(), cpu(0));
    assert!(words.get(cpu(2)).is_none());
}

#[test]
fn from_static_sizes() {
    static SLOTS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
    assert!(Words::from_static(&[]).is_none());
    let words = Words::from_static(&SLOTS).expect("static");
    assert_eq!(words.cpus(), 2);
    let w = words.get(cpu(1)).expect("cpu 1");
    assert_eq!(w.cpu(), cpu(1));
    let raw = Word::new(&SLOTS[1], cpu(1));
    assert_eq!(raw.cpu(), cpu(1));
}
