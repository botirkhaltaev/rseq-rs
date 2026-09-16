//! Private RSEQ word ops. One committing store through the caller word.

use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use crate::abi::{Area, CPU_ID_OFF, CS_OFF, SIG};

pub(crate) enum Attempt {
    Ok(usize),
    Miss(usize),
    Abort,
}

impl Attempt {
    #[inline]
    fn from_status(status: u64, value: usize) -> Self {
        match status {
            0 => Self::Ok(value),
            1 => Self::Miss(value),
            _ => Self::Abort,
        }
    }
}

/// # Safety
/// `area` is this thread's rseq TLS and `word` is a live `AtomicUsize`.
#[inline]
pub(crate) unsafe fn compare_exchange(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    cpu: u32,
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
            "ldr {got:w}, [{rseq}, #{cpu_id_off}]",
            "cmp {got:w}, {cpu:w}",
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
            cpu = in(reg) cpu,
            expect = in(reg) expect,
            new = in(reg) new,
            got = out(reg) _,
            tmp = out(reg) _,
            current = out(reg) current,
            status = lateout(reg) status,
            sig = const SIG,
            cpu_id_off = const CPU_ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, current)
}

/// # Safety
/// `area` is this thread's rseq TLS and `word` is a live `AtomicUsize`.
#[inline]
pub(crate) unsafe fn fetch_add(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    cpu: u32,
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
            "ldr {got:w}, [{rseq}, #{cpu_id_off}]",
            "cmp {got:w}, {cpu:w}",
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
            cpu = in(reg) cpu,
            count = in(reg) count,
            got = out(reg) _,
            tmp = out(reg) _,
            sum = out(reg) _,
            prev = out(reg) prev,
            status = lateout(reg) status,
            sig = const SIG,
            cpu_id_off = const CPU_ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
    }
    Attempt::from_status(status, prev)
}

/// # Safety
/// `area` is this thread's rseq TLS; `word` and `side` are live `AtomicUsize`s.
#[inline]
pub(crate) unsafe fn store_if(
    area: NonNull<Area>,
    word: *mut AtomicUsize,
    cpu: u32,
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
            "b 37f",
            ".inst {sig}",
            "37:",
            "adrp {tmp}, 79b",
            "add {tmp}, {tmp}, :lo12:79b",
            "str {tmp}, [{rseq}, #{cs_off}]",
            "32:",
            "ldr {got:w}, [{rseq}, #{cpu_id_off}]",
            "cmp {got:w}, {cpu:w}",
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
            cpu = in(reg) cpu,
            expect = in(reg) expect,
            new = in(reg) new,
            side = in(reg) side,
            side_new = in(reg) side_new,
            got = out(reg) _,
            tmp = out(reg) _,
            current = out(reg) current,
            status = lateout(reg) status,
            sig = const SIG,
            cpu_id_off = const CPU_ID_OFF,
            cs_off = const CS_OFF,
            options(nostack),
        );
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
