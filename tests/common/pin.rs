//! Thread affinity helper.

/// Pin this thread to `cpu`.
pub(crate) fn pin(cpu: u32) -> bool {
    let cpu = cpu as usize;
    // SAFETY: `cpu_set_t` is stack-local; `CPU_SET` / `sched_setaffinity` take
    // a pointer to that set and a length. No other memory is touched.
    unsafe {
        let mut set = std::mem::zeroed::<libc::cpu_set_t>();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, core::mem::size_of::<libc::cpu_set_t>(), &raw const set) == 0
    }
}
