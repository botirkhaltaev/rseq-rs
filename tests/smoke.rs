//! Bind and fence when rseq is available.

#[path = "common/skip.rs"]
mod skip;

use rseq_rs::Rseq;

use skip::skip;

#[test]
fn bind_when_available() {
    let Some(rseq) = Rseq::new() else {
        skip("rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("glibc area should bind after new");
    let cpu = thread.cpu_id().expect("cpu_id in range after bind");
    assert!(cpu.get() < rseq.cpus());
    if !rseq.fence(cpu) {
        skip("RSEQ membarrier unavailable");
    }
}
