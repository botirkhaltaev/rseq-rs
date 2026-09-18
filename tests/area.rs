//! Remaining `rseq.h` Area reads and drain.

#[path = "common/skip.rs"]
mod skip;

use rseq_rs::{Available, Rseq};

use skip::skip;

#[test]
fn available_kernel() {
    if !Rseq::available(Available::Kernel) {
        skip("SYS_rseq unavailable");
        return;
    }
    if !Rseq::available(Available::Libc) {
        skip("glibc rseq symbols absent");
    }
}

#[test]
fn bind_reads_and_fence_all() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("bind");
    let cpu = thread.cpu_id().expect("cpu_id");
    let start = thread.cpu_id_start().expect("cpu_id_start");
    assert_eq!(thread.cpu(), Some(cpu));
    assert!(start.get() < rseq.cpus());
    if let Some(node) = thread.node_id() {
        assert_eq!(thread.node(), Some(node));
    }
    let _ = thread.slice_ctrl();
    thread.prepare_unload();
    if !rseq.fence_all() {
        skip("RSEQ membarrier unavailable");
        return;
    }
    let again = rseq.bind().expect("rebind after prepare_unload");
    assert!(again.cpu_id().is_some());
}
