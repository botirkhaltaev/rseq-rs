//! CS outcome shared by the arch modules and the fallback stub.

/// Scratch memcpy operands for [`crate::cs::store_if_copy`].
#[derive(Clone, Copy)]
pub(crate) struct Memcpy {
    pub dst: *mut u8,
    pub src: *const u8,
    pub len: usize,
}

pub(crate) enum Attempt {
    Ok(usize),
    Miss(usize),
    Abort,
}

impl Attempt {
    #[inline]
    pub(crate) fn from_status(status: u64, value: usize) -> Self {
        match status {
            0 => Self::Ok(value),
            1 => Self::Miss(value),
            _ => Self::Abort,
        }
    }
}
