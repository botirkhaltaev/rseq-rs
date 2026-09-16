//! Shared test helpers.

/// Pin this thread to `cpu`.
pub fn pin(cpu: u32) -> bool {
    let cpu = cpu as usize;
    unsafe {
        let mut set = std::mem::zeroed::<libc::cpu_set_t>();
        libc::CPU_ZERO(&mut set);
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, core::mem::size_of::<libc::cpu_set_t>(), &raw const set) == 0
    }
}
