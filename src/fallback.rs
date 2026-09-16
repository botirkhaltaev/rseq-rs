//! Stubs when the kernel CS is not implemented on this target.

use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use crate::abi::Area;
use crate::attempt::Attempt;

/// # Safety
/// Unused: this target has no CS.
pub(crate) unsafe fn compare_exchange<const ID_OFF: usize>(
    _area: NonNull<Area>,
    _word: *mut AtomicUsize,
    _id: u32,
    _expect: usize,
    _new: usize,
) -> Attempt {
    Attempt::Abort
}

/// # Safety
/// Unused: this target has no CS.
pub(crate) unsafe fn fetch_add<const ID_OFF: usize>(
    _area: NonNull<Area>,
    _word: *mut AtomicUsize,
    _id: u32,
    _count: usize,
) -> Attempt {
    Attempt::Abort
}

/// # Safety
/// Unused: this target has no CS.
pub(crate) unsafe fn store_if<const ID_OFF: usize>(
    _area: NonNull<Area>,
    _word: *mut AtomicUsize,
    _id: u32,
    _expect: usize,
    _new: usize,
    _side: *mut AtomicUsize,
    _side_new: usize,
) -> Attempt {
    Attempt::Abort
}

pub(crate) struct Registration;

impl Registration {
    pub(crate) fn bind() -> Option<NonNull<Area>> {
        None
    }
}

pub(crate) fn glibc_offset() -> Option<isize> {
    None
}

pub(crate) fn cid_supported() -> bool {
    false
}

pub(crate) fn glibc_area(_offset: isize) -> Option<NonNull<Area>> {
    None
}
