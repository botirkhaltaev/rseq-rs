# AGENTS.md

Scope: this repository.

- Safe public API. `unsafe` only private asm / syscalls / mmap.
- No free helpers. Behavior on `Rseq` / `Thread` / `Word` / `Words` / `Cpus` / `Region` / `Membarrier` / `Registration` / `Cid` / `NodeId` / `Available`.
- Primitive is `Thread` + `Word`. `Words` is an optional mmap. No ops on `Words`.
- One `struct rseq` per thread: glibc's if registered, else the crate's `Registration` TLS, unregistered at thread exit. User-space writes `rseq_cs` only.
- `Word.key` confirms `area.cpu_id` or `area.mm_cid`. `Thread::cpu_id_start` is a read. Do not overlay `cpu_id_start` or use it as the CS key.
- One committing store, last. CS aborts if the field is not `word.key`.
- CS on Linux x86_64 and aarch64. Hit takes `&Thread`. Do not reload `__rseq_offset` / `fs:0` / `tpidr_el0` per op.
- No lock or CAS on the RSEQ hit. No locked twin.
- One attempt. Kernel preemption restarts inside the CS. Index mismatch is `Error::Abort`.
- `new` is glibc rseq or `SYS_rseq`, plus CPU count. `fence` is membarrier RSEQ; word ops never fence.
- Isolated TLS winning a pinned increment is not a bug (`#135`).
- This crate never uses the global allocator. `mmap` is the OS boundary. Cold `new` may use `OnceLock` and `File` into a stack buffer.
- User Rust is not a critical section. `new` is `None` → caller `AtomicUsize`.
- Thesis: `ROADMAP.md`. API: `README.md`.
