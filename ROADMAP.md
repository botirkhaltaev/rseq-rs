# rseq-rs Roadmap

Standalone Linux restartable-sequence primitives for Rust. Package
`rseq-rs`, lib `rseq_rs`. The crate name `rseq` is taken (unrelated DSL).

This is librseq `rseq/rseq.h` in Rust: crate-owned sequences on a
caller-chosen word plus an index check. Not a tcmalloc magazine.
`rseq/mempool.h` is a different API.

As of v1.0 the `rseq.h` catalog is ported. Kernel V2 registration,
a time-slice CS if the kernel adds one, and mempool/magazine stay later.

## Thesis

RSEQ lets a thread run a short sequence of ordinary stores that must look
atomic with respect to preemption and migration. The kernel either lets the
sequence finish on the same CPU or jumps to a signed abort IP. It does not
roll stores back and does not save partial progress. The hit is ordinary
stores — no lock, no CAS.

The primitive is that sequence, not an array. librseq is
`cmpeqv_storev(v, expect, new, cpu)`: the caller owns `v`. This crate
owns the instruction range. The public handle is `Thread` plus `Word`
(`&AtomicUsize` and `CpuId` or `Cid`). `Words` is an optional mmap of words.

The critical section itself is **not** user Rust. A `Fn` / closure /
proc-macro around safe code cannot be a restartable sequence: the compiler
may spill, reorder, or split it, and the kernel needs an exact
`[start_ip, start_ip + post_commit_offset)` range. The crate owns those
sequences.

Compare-miss is `Err(Miss(current))`. Index mismatch is `Err(Abort)`.
Kernel preemption restarts inside the CS. The caller retries abort after
re-reading the index — do not spin on the same `Word`.

Runic integration is out until new thread-heavy gates are named. `#135`
was not a fair test of RSEQ: the impl never reached the tcmalloc hit,
and the gate was single-thread pinned churn (per-CPU cannot win that by
design).

## Done (v1.0)

Registration (glibc or `SYS_rseq`). One `struct rseq` per thread; user
writes `rseq_cs` only. Both index kinds: `cpu_id` and `mm_cid`. The
seven `rseq.h` CS helpers, Area reads (`cpu_id_start`, `node_id`,
`mm_cid`, `slice_ctrl`), `Rseq::available`, `prepare_unload`,
`Rseq::fence` / `fence_all`. Optional `Words`. Linux x86_64 and aarch64.
Other targets compile; `Rseq::new` is `None`.

## Safe API

Behavior lives on the owning types. No free one-liner wrappers.

```rust
let rseq = Rseq::new()?;           // None: kernel / glibc
let t = rseq.bind()?;              // Thread; store in caller TLS
let words = rseq.words()?;         // optional region
let cpu = t.cpu_id()?;
let w = words.get(cpu)?;           // Word { ptr, key }

t.compare_exchange(w, expect, new)?;  // miss / abort
t.fetch_add(w, 1)?;                   // abort
t.store_if(w, expect, new, side, v)?; // scratch then commit
t.compare_exchange_if(w, expect, new, other, other_expect)?;
```

- `Rseq` — process registration. `Copy`. `new` is `#[cold]`, once.
  glibc area if present, else `Registration` + `SYS_rseq`. CPU count.
  `available` does not require `new`. Not membarrier.
- `Thread` — this thread's `Area`. `Copy`. `bind` is `#[cold]`. Owns
  the word ops. Hit takes `&Thread` so it does not reload
  `__rseq_offset` / `fs:0`.
- `Word<K>` — `Copy`. `&AtomicUsize` plus `K: Index`. Defaults to
  `CpuId`. `Words::get` borrows the region. `Word::new` is safe.
- `Words` — optional mmap of one word per possible CPU. Slot count is
  possible CPUs; may be cid-indexed. `get` is address math, not a CS.
  `Rseq::words` maps a new region. `from_static` wraps a `'static` slice.
- `CpuId` — newtype `u32`. `Thread::cpu_id() -> Option<CpuId>`.
- `Cid` — newtype `u32`. Only from `Thread::cid() -> Option<Cid>` when
  the kernel populates `mm_cid`.
- `NodeId` — newtype `u32`. `Thread::node_id() -> Option<NodeId>`.
- `Available` — `Kernel` / `Libc` for `Rseq::available`.
- `Index` — sealed marker. `CpuId` and `Cid` only.
- `Error` — `Miss(usize)` or `Abort`. Exhaustive: the CS has two outcomes.

The CS loads `area.cpu_id` or `area.mm_cid` and aborts if it is not
`word.key`. Then it stores through the borrowed atomic. An index-mismatch abort
does not retry inside the crate: the caller re-reads the index and picks
a new word.

Embedder field in a larger per-CPU struct: `Word::new(&self.head, cpu)`.
Array embedder: `Words::from_static(&SLOTS)`.

`new` is `None` → the kernel has no rseq, or a zero CPU count. The
fallback is the caller's `AtomicUsize`, not a locked twin in this crate.

`Rseq::fence` is optional. First call registers RSEQ membarrier; word ops
never fence. `fence` targets a CPU; a cid word has no CPU. Draining a
cid word is `Rseq::fence_all`.

Other targets: types exist; `new` returns `None`. Dependents compile
everywhere. Word width is `usize` (librseq `intptr_t`). Linux aarch64 uses
the same API as x86_64.

## What #135 got wrong

Tried in [runic#135](https://github.com/botirkhaltaev/runic/issues/135),
reverted. Recorded as churn/64 65.3 vs TLS magazine 43.6. Three impl bugs
and one thesis bug:

1. Intrusive two-store list (payload link, then head). Abort between stores
   is not restartable. Forced `skip_head`, then a `busy` CAS — a lock that
   defeats RSEQ. Fan-in double-freed.
2. Index-stack reshape still rebuilt `rseq_cs` on the stack every pop/push
   (148 → 225 ins/elem). Result 69.3.
3. Static `__rseq_cs` landed, but pop/push stayed outlined, each op paid
   `SeqCst` + `__rseq_offset` + `fs:0`, and free still walked run metadata
   before the CS. Result 65.3. Fan-in still aborted: take/drain had no
   membarrier quiesce.
4. The gate was `taskset -c 0` single-thread churn. A perfect per-CPU pop
   ties a TLS pop plus the `rseq_cs` install. tcmalloc uses per-CPU slabs
   so cache count scales with cores, not threads.

`toccata-core` and `rsmalloc` each reimplemented the same layer as raw
asm + offsets. This crate keeps that CS shape and hides it behind safe
types. The word ops are done. A magazine is a layer, not a new CS.

## Host facts (this machine)

- Kernel 6.12, glibc 2.34 with the RHEL 9 rseq backport.
- `__rseq_offset` / `__rseq_size` / `__rseq_flags` live in `ld.so`.
- glibc `__rseq_size` reports 20. Every kernel registration is 32 bytes.
  Kernel populates `node_id` / `mm_cid`.
- `AT_RSEQ_FEATURE_SIZE` is 28 (`offsetofend(mm_cid)`). That is the
  liveness gate, not `__rseq_size`.
- Self-register via `SYS_rseq` returns `EINVAL` while glibc holds the area.
- `GLIBC_TUNABLES=glibc.pthread.rseq=0` leaves `__rseq_size` 0; v0.4 then
  self-registers.

## ABI contract

The kernel registers **one** per-thread `struct rseq` (`rseq(2)`). glibc
(2.35+, and this host's 2.34 RHEL backport) owns that area when it
registers. A second `SYS_rseq` is `EINVAL`. Libraries share glibc's TLS.
When `__rseq_size` is 0 or the symbols are missing, this crate owns a
32-byte `Registration` TLS area and unregisters it at thread exit.
Every kernel registration is 32 bytes. `__rseq_size` on this glibc is
20 (legacy report); `mm_cid` liveness is `AT_RSEQ_FEATURE_SIZE`.

User-space may write **`rseq_cs` only**. `cpu_id`, `cpu_id_start`,
`node_id`, `mm_cid`, and feature `flags` are kernel-owned. Optimized
RSEQ V2 SIGSEGVs writers of those fields. tcmalloc's cached-slab overlay
on `cpu_id_start` is ABI-hostile and is out of this crate.

`cpu_id_start` is a speculative in-range index. Side effects are legal
only after `cpu_id` confirms it. This crate does not pre-read
`cpu_id_start`. `Word.key` is the librseq confirmation: the CS compares
`area.cpu_id` or `area.mm_cid` to `word.key`.

Remote drain is `membarrier(MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ)`
(Linux 5.10+): abort siblings' CS on a CPU. That is `Rseq::fence`. Word
ops never fence.

An isolated pinned TLS increment beating the CS is not a bug. The extra
cost is the `rseq_cs` install (`#135`). Per-CPU cannot beat TLS on
single-thread pinned churn by design.

```text
cpu = TLS->rseq::cpu_id_start
TLS->rseq::rseq_cs = rseq_cs
[start_ip] if (cpu != TLS->rseq::cpu_id) goto abort_ip
[last instruction = commit]
[post_commit_ip]
```

Time-slice extension and V2 feature-size registration are later kernels.

## Invariants

```text
Safe public API. unsafe only private asm / syscalls / mmap.
One committing store, last. Extra stores before it must be scratch.
Static rseq_cs in __rseq_cs ("aw"), 32-byte aligned. Hit stores the pointer.
Caller-owned Thread. Hit does not load __rseq_offset or fs:0.
CS aborts if the confirmed field is not word.key. Store goes through the atomic.
No per-op fence. No rseq_cs clear after commit (kernel clears on preempt).
RSEQ path never locks or CASes. No locked twin in this crate.
Never GlobalAlloc (no Vec / Box / String / HashMap). mmap is the OS boundary.
Cold paths may use OnceLock and File into a stack buffer.
Workspace lints. missing_docs deny. unsafe_op_in_unsafe_fn deny. Clippy defaults.
```

Crate-owned `Words` may `mmap` / `munmap`. That is the OS boundary, not
an allocator-internal heap.

## Layout

Standalone crate. Word-op impl on `linux + x86_64` and `linux + aarch64`.

```text
src/lib.rs         re-exports
src/rseq.rs        Rseq::new / bind / fence / words
src/thread.rs      Thread, CpuId, Cid, NodeId, Index, word ops
src/registration.rs Registration (self-register TLS)
src/fallback.rs    CS and Registration stubs
src/attempt.rs     CS Attempt
src/words.rs       Word, Words
src/region.rs      Region (mmap or caller span)
src/cpus.rs        Cpus (sysfs possible)
src/membarrier.rs  Membarrier (fence)
src/x86_64.rs      private inline asm! (not pub)
src/aarch64.rs     private inline asm! (not pub)
src/abi.rs         private Area / SIG
benches/counter.rs librseq addv; retry-loop + bare Word
benches/cached.rs  tcmalloc 1-deep; retry-loop + bare Word
benches/freelist.rs librseq / mempool stack
benches/drain.rs   tcmalloc FenceCpu + steal
```

`Words::get` is `base + cpu * size_of::<AtomicUsize>()`. Zeros on crate `mmap`.

`asm!` shape: `.pushsection __rseq_cs,"aw"` + local labels (PIE-safe; no
`global_asm!` outline). Abort signature immediately before the abort IP
(`.long SIG` on x86_64, `.inst SIG` on aarch64). Then `lea` / `adrp`+`add`
into `area.rseq_cs`. Load `cpu_id` or `mm_cid`; abort if not `word.key`.
Compare-exchange or add through the word. Committing store last. Kernel
abort restarts at the signed IP. Index mismatch returns `Error::Abort`.
No `cpu_id_start` pre-read and recheck.

## Releases

### v0.1.0 — x86_64 word ops (released)

```text
Rseq / Thread / CpuId / Word / Words / Error
Thread::compare_exchange / fetch_add; Word::new
tests: abi (private), smoke, words, ops, stress (ignored)
bench: counter (addv + bare Word), cached (take/put + bare Word),
       freelist (librseq list), drain (FenceCpu)
README + AGENTS.md
```

Stress: threads > cores, `sched_setaffinity` flap + `setitimer` SIGALRM.
Unique add / no lost CAS. A signature bug is SIGSEGV.

Retry-loop benches are the real caller (`cpu_id` + `get` each iter).
Pinned `*_word` benches reuse one `Word` so the CS number is visible.
`#135` never isolated either. Do not chase `#[inline(always)]`,
fall-through status, or `addq` vs `xadd` without a new isolated table.

### v0.2.0 — aarch64 (released)

Same safe API. `adrp`/`add` for `cs`, `mrs tpidr_el0`, aarch64 `SIG`
(`BRK #0x45E0`). CI `cargo check --target aarch64-unknown-linux-gnu`.

### v0.3.0 — `store_if`

librseq `cmpeqv_trystorev_storev`. Two `Word`s: compare `word` to
`expect`, scratch-store `side`, commit-store `word`. `side.cpu` must
equal `word.key` or `Abort` (Rust, before the CS). The CS still confirms
only `word.key`. Still one committing store. No user closure.

### v0.4.0 — self-registration

Always on. `dlsym` `__rseq_offset` / `__rseq_size`. Size >= 20 uses
glibc's area. Else `Registration` TLS, `SYS_rseq` 32-byte area,
unregister on thread exit. `new` is `None` only if the kernel has no
rseq.

### v0.5.0 — `mm_cid` (word-op primitive done)

`Cid` and sealed `Index`. `Thread::cid()` when `AT_RSEQ_FEATURE_SIZE >=
28`. `Word<K>` / word ops confirm `area.mm_cid` or `area.cpu_id` via one
const offset. `Cid` only from `Thread::cid`. `Word::new` is safe.
`Rseq::new`. Overlay of `cpu_id_start` stays out.

### v0.6.0 — remaining `rseq.h` reads

`NodeId`, `Thread::cpu_id_start` (read only), `Thread::cpu` /
`Thread::node` fallbacks, `Thread::prepare_unload`, `Thread::slice_ctrl`,
`Rseq::available`, `Rseq::fence_all`.

### v0.7.0 — dual compare

`Thread::compare_exchange_if` (`rseq_load_cbne_load_cbne_store`).

### v0.8.0 — pointer chases

`Thread::load_if_ne` and `Thread::fetch_add_at`. Byte offsets, same CS
as C. Not rewritten as a second `Word`.

### v0.9.0 — memcpy and Release

`Thread::store_if_copy` (cap `COPY_MAX`). `store_if_release` /
`store_if_copy_release`.

### v1.0.0 — freeze

librseq `rseq.h` port complete. README map. CS catalog frozen.

## Later

```text
RSEQ V2 registration size
time-slice CS if the kernel adds one
mempool / magazine as a different crate or later layer
```

## Out

```text
User Rust / closures / proc-macros as the critical section
Silent lock or CAS on the RSEQ hit
Locked / atomic twin in this crate
Ops on Words
Overlay or write of cpu_id_start / other kernel-owned Area fields
Two committing stores in one CS (tcmalloc slab; #135)
rseq/mempool.h (magazine / per-CPU allocator)
Runic magazine / heap / PageMap
Porting tcmalloc or snmalloc
crates.io publish until benches and stress are green
Runic integration before the isolated bench table exists
```

## Integration (later)

Starts from the v0.1 bench table and new gates: threads > cores, thread
spawn churn, RSS under thread count. Not churn/64. Not a retry of `#135`
as written. Runic would call `Word::new` on a field or own `Words`.
