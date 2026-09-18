//! Private RSEQ word ops. One committing store through the caller word.

use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use crate::abi::{Area, CS_OFF, SIG};
use crate::attempt::Attempt;

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
            "jmp 17f",
            ".long {sig}",
            "17:",
            "lea {tmp}, [rip + 99b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "12:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 19f",
            "mov {current}, qword ptr [{word}]",
            "cmp {current}, {expect}",
            "jne 18f",
            "mov qword ptr [{word}], {new}",
            "16:",
            "xor {status:e}, {status:e}",
            "jmp 30f",
            "18:",
            "mov {status:e}, 1",
            "jmp 30f",
            "19:",
            "xor {current:e}, {current:e}",
            "mov {status:e}, 2",
            "30:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            expect = in(reg) expect,
            new = in(reg) new,
            got = out(reg) _,
            tmp = out(reg) _,
            current = out(reg) current,
            status = out(reg) status,
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
    // `xadd` is the single committing RMW; old value lands in `prev`.
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
            "jmp 27f",
            ".long {sig}",
            "27:",
            "lea {tmp}, [rip + 89b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "22:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 29f",
            "mov {prev}, {count}",
            "xadd qword ptr [{word}], {prev}",
            "26:",
            "xor {status:e}, {status:e}",
            "jmp 20f",
            "29:",
            "xor {prev:e}, {prev:e}",
            "mov {status:e}, 2",
            "20:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            count = in(reg) count,
            got = out(reg) _,
            tmp = out(reg) _,
            prev = out(reg) prev,
            status = out(reg) status,
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
pub(crate) unsafe fn store_if<const ID_OFF: usize>(
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
            "jmp 37f",
            ".long {sig}",
            "37:",
            "lea {tmp}, [rip + 79b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "32:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 39f",
            "mov {current}, qword ptr [{word}]",
            "cmp {current}, {expect}",
            "jne 38f",
            "mov qword ptr [{side}], {side_new}",
            "mov qword ptr [{word}], {new}",
            "36:",
            "xor {status:e}, {status:e}",
            "jmp 40f",
            "38:",
            "mov {status:e}, 1",
            "jmp 40f",
            "39:",
            "xor {current:e}, {current:e}",
            "mov {status:e}, 2",
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
            status = out(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
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
            "jmp 57f",
            ".long {sig}",
            "57:",
            "lea {tmp}, [rip + 59b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "52:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 55f",
            "mov {current}, qword ptr [{word}]",
            "cmp {current}, {expect}",
            "jne 54f",
            "mov {other_val}, qword ptr [{other}]",
            "cmp {other_val}, {other_expect}",
            "jne 53f",
            "mov qword ptr [{word}], {new}",
            "56:",
            "xor {status:e}, {status:e}",
            "jmp 50f",
            "53:",
            "mov {current}, {other_val}",
            "54:",
            "mov {status:e}, 1",
            "jmp 50f",
            "55:",
            "xor {current:e}, {current:e}",
            "mov {status:e}, 2",
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
            status = out(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` is a live `AtomicUsize`.
/// If `*word != expect_not`, `*word + offset` is a live readable `usize`.
#[inline]
pub(crate) unsafe fn load_if_ne<const ID_OFF: usize>(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    id: u32,
    expect_not: usize,
    offset: isize,
) -> Attempt {
    let current: usize;
    let status: u64;
    // SAFETY: `area` is this thread's rseq TLS; `word` is live; the chase is
    // caller-valid when the compare passes.
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
            "jmp 47f",
            ".long {sig}",
            "47:",
            "lea {tmp}, [rip + 49b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "42:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 45f",
            "mov {current}, qword ptr [{word}]",
            "cmp {current}, {expect_not}",
            "je 44f",
            "mov {chase}, {current}",
            "add {chase}, {offset}",
            "mov {chase}, qword ptr [{chase}]",
            "mov qword ptr [{word}], {chase}",
            "46:",
            "xor {status:e}, {status:e}",
            "jmp 41f",
            "44:",
            "mov {status:e}, 1",
            "jmp 41f",
            "45:",
            "xor {current:e}, {current:e}",
            "mov {status:e}, 2",
            "41:",
            rseq = in(reg) area.as_ptr(),
            word = in(reg) word,
            id = in(reg) id,
            expect_not = in(reg) expect_not,
            offset = in(reg) offset,
            got = out(reg) _,
            tmp = out(reg) _,
            chase = out(reg) _,
            current = out(reg) current,
            status = out(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS; `ptr` is live; `*ptr + offset` is a live
/// pointer to a word.
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
    // `xadd` through the chased pointer is the committing RMW.
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
            "jmp 67f",
            ".long {sig}",
            "67:",
            "lea {tmp}, [rip + 69b]",
            "mov qword ptr [{rseq} + {cs_off}], {tmp}",
            "62:",
            "mov {got:e}, dword ptr [{rseq} + {id_off}]",
            "cmp {got:e}, {id:e}",
            "jne 65f",
            "mov {base}, qword ptr [{ptr}]",
            "add {base}, {offset}",
            "mov {slot}, qword ptr [{base}]",
            "mov {prev}, {count}",
            "xadd qword ptr [{slot}], {prev}",
            "66:",
            "xor {status:e}, {status:e}",
            "jmp 61f",
            "65:",
            "xor {prev:e}, {prev:e}",
            "mov {status:e}, 2",
            "61:",
            rseq = in(reg) area.as_ptr(),
            ptr = in(reg) ptr,
            id = in(reg) id,
            offset = in(reg) offset,
            count = in(reg) count,
            got = out(reg) _,
            tmp = out(reg) _,
            base = out(reg) _,
            slot = out(reg) _,
            prev = out(reg) prev,
            status = out(reg) status,
            sig = const SIG,
            id_off = const ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, prev)
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
                "lea {p}, [rip + 7f]",
                "mov {seen:e}, dword ptr [{p} - 4]",
                "jmp 8f",
                "jmp 7f",
                ".long {sig}",
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
