//! Possible-CPU count from sysfs. Stack buffer only — no `String`.

use std::{fs::File, io::Read};

const PATH: &str = "/sys/devices/system/cpu/possible";

/// Highest listed possible CPU id plus one (`0-95` → 96).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cpus(u32);

impl Cpus {
    /// `None` if sysfs is missing, empty, or the count is zero.
    pub(crate) fn possible() -> Option<Self> {
        let mut buf = [0u8; 256];
        let n = File::open(PATH).ok()?.read(&mut buf).ok()?;
        let raw = core::str::from_utf8(buf.get(..n)?).ok()?.trim();
        if raw.is_empty() {
            return None;
        }
        let mut max = 0u32;
        for part in raw.split(',') {
            let hi = if let Some((lo, hi)) = part.split_once('-') {
                let _: u32 = lo.parse().ok()?;
                hi.parse().ok()?
            } else {
                part.parse().ok()?
            };
            max = max.max(hi);
        }
        Some(Self(max.checked_add(1)?))
    }

    pub(crate) const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Cpus;

    #[test]
    fn possible() {
        assert!(Cpus::possible().is_some_and(|c| c.get() > 0));
    }
}
