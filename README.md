# rseq-rs

Safe Linux [restartable sequence](https://google.github.io/tcmalloc/rseq.html)
word ops. Package `rseq-rs`, lib `rseq_rs`. librseq in Rust — crate-owned
sequences on a caller-chosen word. Not a runic hit path, not a magazine.

## Registration

```rust
use rseq_rs::{Available, Rseq};

assert!(Rseq::available(Available::Kernel));
let rseq = Rseq::new()?;
let t = rseq.bind()?;
let cpu = t.cpu_id()?;
assert!(cpu.get() < rseq.cpus());
assert_eq!(t.cpu(), cpu);
let _ = t.cpu_id_start();
let _ = t.node_id();
let _ = t.slice_ctrl();
let _ = rseq.fence(cpu);
let _ = rseq.fence_all();
t.prepare_unload();
```

`new` / `bind` are `#[cold]`. Store `Thread` in caller TLS. `None` means
the kernel has no rseq — use `AtomicUsize`, not a hidden lock here. glibc's
area is used when present; otherwise the crate registers a 32-byte TLS
area and unregisters it at thread exit. `fence` is optional and registers
membarrier itself. It targets a CPU; drain a cid word with `fence_all`.

## Words region

```rust
use rseq_rs::Words;

let words = Words::new(2)?;
let w = words.get(cpu)?;
```

`get` borrows `words` — keep the region alive. Embedder field:
`Word::new(&atomic, cpu)`. Slot count is possible CPUs; `mm_cid` is `<`
allowed CPUs `<=` possible CPUs, so `rseq.words()` covers cids.

## Word ops (Linux x86-64 / aarch64)

```rust
use rseq_rs::{Error, Rseq};

let rseq = Rseq::new()?;
let t = rseq.bind()?;
let words = rseq.words()?;
loop {
    let cpu = t.cpu_id()?;
    let w = words.get(cpu)?;
    match t.compare_exchange(w, 0, 7) {
        Ok(_) | Err(Error::Miss(_)) => break,
        Err(Error::Abort) => {}
    }
}
```

`store_if(word, expect, new, side, side_new)` is librseq
`cmpeqv_trystorev_storev`: scratch store to `side`, then one committing
store to `word`. `side.key` must equal `word.key`.

On kernels that populate `mm_cid`, index by cid instead of `cpu_id`:

```rust
let cid = t.cid()?;
let w = words.get(cid)?;
```

One attempt per call. Kernel preemption restarts inside the CS. Index
mismatch is `Err(Abort)` — re-read `cpu_id` or `cid` and pick again. Do
not retry the same `Word`. Compare-miss is `Err(Miss(current))`. No lock,
no CAS.

A bad abort signature is SIGSEGV, not `Error::Abort`.

Stress (ignored): `cargo test -- --ignored`. Tests that need rseq, two
CPUs, a pin, or `mm_cid` print `skip:` and pass when the host cannot
run them.

Use-case benches (one file each). Isolated numbers: `taskset -c 0`.
`counter` / `cached` report a bare-`Word` CS next to the retry-loop caller.

```text
cargo bench --bench counter    # librseq addv / per-CPU stats
cargo bench --bench cached     # tcmalloc 1-deep cached object
cargo bench --bench freelist   # librseq / mempool per-CPU stack
cargo bench --bench drain      # tcmalloc FenceCpu + steal
```

Each file reports rseq next to a non-rseq pair (TLS `Cell` and/or `AtomicUsize`).
Non-rseq benches still run if rseq is unavailable.

See [ROADMAP.md](ROADMAP.md). `publish = false`.
