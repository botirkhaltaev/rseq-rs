//! CS outcome shared by the arch modules and the fallback stub.

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
