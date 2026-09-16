# rseq-rs

Safe Linux [restartable sequence](https://google.github.io/tcmalloc/rseq.html)
word ops. Package `rseq-rs`, lib `rseq_rs`. An idiomatic Rust port of
librseq [`rseq/rseq.h`](https://github.com/compudj/librseq/blob/master/include/rseq/rseq.h):
crate-owned sequences on a caller-chosen word. Not a runic hit path, not
a magazine (`rseq/mempool.h` is a different API).

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
store to `word`. `side.key` must equal `word.key`. `store_if_release` is
the same sequence with a Release commit.

`compare_exchange_if` compares two words, then stores one.
`load_if_ne` / `fetch_add_at` chase a byte offset from a loaded pointer
(same CS as C; a wrong offset is the caller's bug).
`store_if_copy` memcpy-scratches up to `COPY_MAX` bytes, then commits.

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

## librseq `rseq.h` map

| librseq | this crate |
| --- | --- |
| `rseq_init` | `Rseq::new` |
| `rseq_available` | `Rseq::available` |
| `rseq_registered` | `Rseq::new` / `bind` succeed |
| `rseq_get_abi` / `rseq_offset` / `rseq_size` | private (`Thread`) |
| `rseq_current_cpu_raw` | `Thread::cpu_id` |
| `rseq_cpu_start` | `Thread::cpu_id_start` (read only) |
| `rseq_current_cpu` | `Thread::cpu` |
| `rseq_fallback_current_cpu` | private (`cpu`) |
| `rseq_current_node_id` / `rseq_node_id_available` | `Thread::node_id` |
| `rseq_fallback_current_node` | private (`node`) |
| `rseq_current_mm_cid` / `rseq_mm_cid_available` | `Thread::cid` |
| `rseq_slice_ctrl_available` | `Thread::slice_ctrl` |
| `rseq_prepare_unload` / `rseq_clear_rseq_cs` | `Thread::prepare_unload` |
| `rseq_load_cbne_store` (`cmpeqv_storev`) | `Thread::compare_exchange` |
| `rseq_load_add_store` (`addv`) | `Thread::fetch_add` |
| `rseq_load_cbne_store_store` Relaxed | `Thread::store_if` |
| `rseq_load_cbne_store_store` Release | `Thread::store_if_release` |
| `rseq_load_cbne_load_cbne_store` | `Thread::compare_exchange_if` |
| `rseq_load_cbeq_store_add_load_store` | `Thread::load_if_ne` |
| `rseq_load_add_load_load_add_store` | `Thread::fetch_add_at` |
| `rseq_load_cbne_memcpy_store` Relaxed | `Thread::store_if_copy` |
| `rseq_load_cbne_memcpy_store` Release | `Thread::store_if_copy_release` |
| `rseq_get_max_nr_cpus` | mempool-adjacent; not ported |
| `rseq/mempool.h` | not the rseq API |

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
