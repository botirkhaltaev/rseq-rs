//! Bind and fence when rseq is available.

use rseq_rs::Rseq;

#[test]
fn bind_when_available() {
    let Some(rseq) = Rseq::new() else {
        eprintln!("skip: rseq unavailable");
        return;
    };
    let thread = rseq.bind().expect("glibc area should bind after new");
    let cpu = thread.cpu_id().expect("cpu_id in range after bind");
    assert!(cpu.get() < rseq.cpus());
    assert!(rseq.fence(cpu));
}
