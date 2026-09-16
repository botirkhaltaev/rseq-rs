//! Remaining `rseq.h` Area reads and drain.

use rseq_rs::{Available, Rseq};

#[test]
fn available_kernel() {
    assert!(Rseq::available(Available::Kernel), "this host has SYS_rseq");
    assert!(
        Rseq::available(Available::Libc),
        "this host exports glibc rseq"
    );
}

#[test]
fn bind_reads_and_fence_all() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu_id");
    let start = thread.cpu_id_start().expect("cpu_id_start");
    assert_eq!(thread.cpu(), cpu);
    assert!(start.get() < rseq.cpus());
    if let Some(node) = thread.node_id() {
        assert_eq!(thread.node(), node);
    }
    let _ = thread.slice_ctrl();
    thread.prepare_unload();
    assert!(rseq.fence_all());
    let again = rseq.bind().expect("rebind after prepare_unload");
    assert!(again.cpu_id().is_some());
}
