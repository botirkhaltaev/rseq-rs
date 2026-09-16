//! Private RSEQ word ops. One committing store through the caller word.

use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use crate::abi::{Area, CS_OFF, SIG};
use crate::attempt::{Attempt, Memcpy};

/// # Safety
/// `area` is this thread's rseq TLS and `word` is a live `AtomicUsize`.
#[inline]
pub(crate) unsafe fn compare_exchange<const ID_OFF: usize>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect: usize,
    new: usize,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; `word` is a live usize.
    unsafe {
        core::arch::asm!(
            ".pushsection __rseq_cs, \"aw\"",
            ".balign 32",
            "99:",
            ".long 0",
            ".long 0",
            ".quad 12f",
            ".quad (16f - 12f)",
            ".quad 17f",
            ".popsection",
            "b 17f",
            ".inst {sig}",
            "17:",
            "adrp {tmp}, 99b",
            "add {tmp}, {tmp}, :lo12:99b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "12:",
            "ldr {got:w}, [{rseq}, #{id_off}]",
            "cmp {got:w}, {id:w}",
            "b.ne 19f",
            "ldr {current}, [{word}]",
            "cmp {current}, {expect}",
            "b.ne 18f",
            "str {new}, [{word}]",
            "16:",
            "mov {status}, xzr",
            "b 30f",
            "18:",
            "mov {status}, #1",
            "b 30f",
            "19:",
            "mov {current}, xzr",
            "mov {status}, #2",
            "30:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            expect = in(reg) expect,
            new = in(reg) new,
            got = out(reg) _,
            tmp = out(reg) _,
            current = out(reg) current,
            status = lateout(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS and `word` is a live `AtomicUsize`.
#[inline]
pub(crate) unsafe fn fetch_add<const ID_OFF: usize>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    count: usize,
) -> Attempt {
    let prev: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; `word` is a live AtomicUsize.
    // `ldr` / `add` are scratch; `str` of the sum is the committing store.
    unsafe {
        core::arch::asm!(
            ".pushsection __rseq_cs, \"aw\"",
            ".balign 32",
            "89:",
            ".long 0",
            ".long 0",
            ".quad 22f",
            ".quad (26f - 22f)",
            ".quad 27f",
            ".popsection",
            "b 27f",
            ".inst {sig}",
            "27:",
            "adrp {tmp}, 89b",
            "add {tmp}, {tmp}, :lo12:89b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "22:",
            "ldr {got:w}, [{rseq}, #{id_off}]",
            "cmp {got:w}, {id:w}",
            "b.ne 29f",
            "ldr {prev}, [{word}]",
            "add {sum}, {prev}, {count}",
            "str {sum}, [{word}]",
            "26:",
            "mov {status}, xzr",
            "b 20f",
            "29:",
            "mov {prev}, xzr",
            "mov {status}, #2",
            "20:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            count = in(reg) count,
            got = out(reg) _,
            tmp = out(reg) _,
            sum = out(reg) _,
            prev = out(reg) prev,
            status = lateout(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, prev)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` and `side` are live `AtomicUsize`s.
#[inline]
pub(crate) unsafe fn store_if<const ID_OFF: usize, const RELEASE: bool>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect: usize,
    new: usize,
    side: *mut AtomicUsize,
    side_new: usize,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; both words are live usizes.
    // The store to `side` is scratch; the store to `word` is the commit.
    unsafe {
        if RELEASE {
            core::arch::asm!(
                ".pushsection __rseq_cs, \"aw\"",
                ".balign 32",
                "79:",
                ".long 0",
                ".long 0",
                ".quad 32f",
                ".quad (36f - 32f)",
                ".quad 37f",
                ".popsection",
                "b 37f",
                ".inst {sig}",
                "37:",
                "adrp {tmp}, 79b",
                "add {tmp}, {tmp}, :lo12:79b",
                "str {tmp}, [{rseq}, #{cs_off}]",
                "32:",
                "ldr {got:w}, [{rseq}, #{id_off}]",
                "cmp {got:w}, {id:w}",
                "b.ne 39f",
                "ldr {current}, [{word}]",
                "cmp {current}, {expect}",
                "b.ne 38f",
                "str {side_new}, [{side}]",
                "stlr {new}, [{word}]",
                "36:",
                "mov {status}, xzr",
                "b 40f",
                "38:",
                "mov {status}, #1",
                "b 40f",
                "39:",
                "mov {current}, xzr",
                "mov {status}, #2",
                "40:",
                rseq = in(reg) area.as_ptr(),
                word = in(reg) word,
                id = in(reg) id,
                expect = in(reg) expect,
                new = in(reg) new,
                side = in(reg) side,
                side_new = in(reg) side_new,
                got = out(reg) _,
                tmp = out(reg) _,
                current = out(reg) current,
                status = lateout(reg) status,
                sig = const SIG,
                id_off = const ID_OFF,
                cs_off = const CS_OFF,
                options(nostack),
            );
        } else {
            core::arch::asm!(
                ".pushsection __rseq_cs, \"aw\"",
                ".balign 32",
                "79:",
                ".long 0",
                ".long 0",
                ".quad 32f",
                ".quad (36f - 32f)",
                ".quad 37f",
                ".popsection",
                "b 37f",
                ".inst {sig}",
                "37:",
                "adrp {tmp}, 79b",
                "add {tmp}, {tmp}, :lo12:79b",
                "str {tmp}, [{rseq}, #{cs_off}]",
                "32:",
                "ldr {got:w}, [{rseq}, #{id_off}]",
                "cmp {got:w}, {id:w}",
                "b.ne 39f",
                "ldr {current}, [{word}]",
                "cmp {current}, {expect}",
                "b.ne 38f",
                "str {side_new}, [{side}]",
                "str {new}, [{word}]",
                "36:",
                "mov {status}, xzr",
                "b 40f",
                "38:",
                "mov {status}, #1",
                "b 40f",
                "39:",
                "mov {current}, xzr",
                "mov {status}, #2",
                "40:",
                rseq = in(reg) area.as_ptr(),
                word = in(reg) word,
                id = in(reg) id,
                expect = in(reg) expect,
                new = in(reg) new,
                side = in(reg) side,
                side_new = in(reg) side_new,
                got = out(reg) _,
                tmp = out(reg) _,
                current = out(reg) current,
                status = lateout(reg) status,
                sig = const SIG,
                id_off = const ID_OFF,
                cs_off = const CS_OFF,
                options(nostack),
            );
        }
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` and `other` are live `AtomicUsize`s.
#[inline]
pub(crate) unsafe fn compare_exchange_if<const ID_OFF: usize>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect: usize,
    new: usize,
    other: *mut AtomicUsize,
    other_expect: usize,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; both words are live usizes.
    unsafe {
        core::arch::asm!(
            ".pushsection __rseq_cs, \"aw\"",
            ".balign 32",
            "59:",
            ".long 0",
            ".long 0",
            ".quad 52f",
            ".quad (56f - 52f)",
            ".quad 57f",
            ".popsection",
            "b 57f",
            ".inst {sig}",
            "57:",
            "adrp {tmp}, 59b",
            "add {tmp}, {tmp}, :lo12:59b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "52:",
            "ldr {got:w}, [{rseq}, #{id_off}]",
            "cmp {got:w}, {id:w}",
            "b.ne 55f",
            "ldr {current}, [{word}]",
            "cmp {current}, {expect}",
            "b.ne 54f",
            "ldr {other_val}, [{other}]",
            "cmp {other_val}, {other_expect}",
            "b.ne 53f",
            "str {new}, [{word}]",
            "56:",
            "mov {status}, xzr",
            "b 50f",
            "53:",
            "mov {current}, {other_val}",
            "54:",
            "mov {status}, #1",
            "b 50f",
            "55:",
            "mov {current}, xzr",
            "mov {status}, #2",
            "50:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            expect = in(reg) expect,
            new = in(reg) new,
            other = in(reg) other,
            other_expect = in(reg) other_expect,
            got = out(reg) _,
            tmp = out(reg) _,
            other_val = out(reg) _,
            current = out(reg) current,
            status = lateout(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` is live; `*word + offset` is a live word pointer.
#[inline]
pub(crate) unsafe fn load_if_ne<const ID_OFF: usize>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect_not: usize,
    offset: isize,
    out: *mut AtomicUsize,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; `word`/`out` are live; chase is caller-valid.
    unsafe {
        core::arch::asm!(
            ".pushsection __rseq_cs, \"aw\"",
            ".balign 32",
            "49:",
            ".long 0",
            ".long 0",
            ".quad 42f",
            ".quad (46f - 42f)",
            ".quad 47f",
            ".popsection",
            "b 47f",
            ".inst {sig}",
            "47:",
            "adrp {tmp}, 49b",
            "add {tmp}, {tmp}, :lo12:49b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "42:",
            "ldr {got:w}, [{rseq}, #{id_off}]",
            "cmp {got:w}, {id:w}",
            "b.ne 45f",
            "ldr {current}, [{word}]",
            "cmp {current}, {expect_not}",
            "b.eq 44f",
            "str {current}, [{out}]",
            "add {chase}, {current}, {offset}",
            "ldr {chase}, [{chase}]",
            "str {chase}, [{word}]",
            "46:",
            "mov {status}, xzr",
            "b 40f",
            "44:",
            "mov {status}, #1",
            "b 40f",
            "45:",
            "mov {current}, xzr",
            "mov {status}, #2",
            "40:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            expect_not = in(reg) expect_not,
            offset = in(reg) offset,
            out = in(reg) out,
            got = out(reg) _,
            tmp = out(reg) _,
            chase = out(reg) _,
            current = out(reg) current,
            status = lateout(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS; `ptr` is live; `*ptr + offset` is a live pointer to a word.
#[inline]
pub(crate) unsafe fn fetch_add_at<const ID_OFF: usize>(
    area: NonNull<Area>,
    ptr: *mut AtomicUsize,
    id: u32,
    offset: isize,
    count: usize,
) -> Attempt {
    let prev: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; the chase is caller-valid.
    // `ldr`/`add` are scratch; `str` of the sum is the committing store.
    unsafe {
        core::arch::asm!(
            ".pushsection __rseq_cs, \"aw\"",
            ".balign 32",
            "69:",
            ".long 0",
            ".long 0",
            ".quad 62f",
            ".quad (66f - 62f)",
            ".quad 67f",
            ".popsection",
            "b 67f",
            ".inst {sig}",
            "67:",
            "adrp {tmp}, 69b",
            "add {tmp}, {tmp}, :lo12:69b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "62:",
            "ldr {got:w}, [{rseq}, #{id_off}]",
            "cmp {got:w}, {id:w}",
            "b.ne 65f",
            "ldr {base}, [{ptr}]",
            "add {base}, {base}, {offset}",
            "ldr {slot}, [{base}]",
            "ldr {prev}, [{slot}]",
            "add {sum}, {prev}, {count}",
            "str {sum}, [{slot}]",
            "66:",
            "mov {status}, xzr",
            "b 60f",
            "65:",
            "mov {prev}, xzr",
            "mov {status}, #2",
            "60:",
            rseq = in(reg) area.as_ptr(),
            ptr = in(reg) ptr,
            id = in(reg) id,
            offset = in(reg) offset,
            count = in(reg) count,
            got = out(reg) _,
            tmp = out(reg) _,
            base = out(reg) _,
            slot = out(reg) _,
            sum = out(reg) _,
            prev = out(reg) prev,
            status = lateout(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, prev)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` is live; `dst`/`src` are `len` live bytes.
#[inline]
pub(crate) unsafe fn store_if_copy<const ID_OFF: usize, const RELEASE: bool>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect: usize,
    new: usize,
    copy: Memcpy,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; memcpy is scratch; store to `word` commits.
    unsafe {
        if RELEASE {
            core::arch::asm!(
                ".pushsection __rseq_cs, \"aw\"",
                ".balign 32",
                "29:",
                ".long 0",
                ".long 0",
                ".quad 22f",
                ".quad (26f - 22f)",
                ".quad 27f",
                ".popsection",
                "b 27f",
                ".inst {sig}",
                "27:",
                "adrp {tmp}, 29b",
                "add {tmp}, {tmp}, :lo12:29b",
                "str {tmp}, [{rseq}, #{cs_off}]",
                "22:",
                "ldr {got:w}, [{rseq}, #{id_off}]",
                "cmp {got:w}, {id:w}",
                "b.ne 25f",
                "ldr {current}, [{word}]",
                "cmp {current}, {expect}",
                "b.ne 24f",
                "cbz {len}, 23f",
                "21:",
                "ldrb {byte:w}, [{src}]",
                "strb {byte:w}, [{dst}]",
                "add {src}, {src}, #1",
                "add {dst}, {dst}, #1",
                "subs {len}, {len}, #1",
                "b.ne 21b",
                "23:",
                "stlr {new}, [{word}]",
                "26:",
                "mov {status}, xzr",
                "b 20f",
                "24:",
                "mov {status}, #1",
                "b 20f",
                "25:",
                "mov {current}, xzr",
                "mov {status}, #2",
                "20:",
                rseq = in(reg) area.as_ptr(),
                word = in(reg) word,
                id = in(reg) id,
                expect = in(reg) expect,
                new = in(reg) new,
                dst = inout(reg) copy.dst => _,
                src = inout(reg) copy.src => _,
                len = inout(reg) copy.len => _,
                got = out(reg) _,
                tmp = out(reg) _,
                byte = out(reg) _,
                current = out(reg) current,
                status = lateout(reg) status,
                sig = const SIG,
                id_off = const ID_OFF,
                cs_off = const CS_OFF,
                options(nostack),
            );
        } else {
            core::arch::asm!(
                ".pushsection __rseq_cs, \"aw\"",
                ".balign 32",
                "29:",
                ".long 0",
                ".long 0",
                ".quad 22f",
                ".quad (26f - 22f)",
                ".quad 27f",
                ".popsection",
                "b 27f",
                ".inst {sig}",
                "27:",
                "adrp {tmp}, 29b",
                "add {tmp}, {tmp}, :lo12:29b",
                "str {tmp}, [{rseq}, #{cs_off}]",
                "22:",
                "ldr {got:w}, [{rseq}, #{id_off}]",
                "cmp {got:w}, {id:w}",
                "b.ne 25f",
                "ldr {current}, [{word}]",
                "cmp {current}, {expect}",
                "b.ne 24f",
                "cbz {len}, 23f",
                "21:",
                "ldrb {byte:w}, [{src}]",
                "strb {byte:w}, [{dst}]",
                "add {src}, {src}, #1",
                "add {dst}, {dst}, #1",
                "subs {len}, {len}, #1",
                "b.ne 21b",
                "23:",
                "str {new}, [{word}]",
                "26:",
                "mov {status}, xzr",
                "b 20f",
                "24:",
                "mov {status}, #1",
                "b 20f",
                "25:",
                "mov {current}, xzr",
                "mov {status}, #2",
                "20:",
                rseq = in(reg) area.as_ptr(),
                word = in(reg) word,
                id = in(reg) id,
                expect = in(reg) expect,
                new = in(reg) new,
                dst = inout(reg) copy.dst => _,
                src = inout(reg) copy.src => _,
                len = inout(reg) copy.len => _,
                got = out(reg) _,
                tmp = out(reg) _,
                byte = out(reg) _,
                current = out(reg) current,
                status = lateout(reg) status,
                sig = const SIG,
                id_off = const ID_OFF,
                cs_off = const CS_OFF,
                options(nostack),
            );
        }
    }
    Attempt::from_status(status, current)
}

#[cfg(test)]
mod tests {
    use crate::abi::SIG;

    #[test]
    fn signature_precedes_abort() {
        let seen: u32;
        // SAFETY: the block only reads four bytes immediately before a local label.
        unsafe {
            core::arch::asm!(
                "adr {p}, 7f",
                "ldr {seen:w}, [{p}, #-4]",
                "b 8f",
                "b 7f",
                ".inst {sig}",
                "7:",
                "8:",
                p = out(reg) _,
                seen = out(reg) seen,
                sig = const SIG,
                options(nostack),
            );
        }
        assert_eq!(seen, SIG);
    }
}
