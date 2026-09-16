//! Safe Linux restartable-sequence primitives.
//!
//! [`Rseq`] registration and [`Thread`] word ops. Keep [`Words`] alive across
//! [`Words::get`]; on [`Error::Abort`] re-read [`Thread::cpu_id`] or
//! [`Thread::cid`] and pick a new word — do not retry the same [`Word`].
//!
//! ```
//! # fn try_it() -> Option<()> {
//! use rseq_rs::{Error, Rseq};
//! let rseq = Rseq::new()?;
//! let t = rseq.bind()?;
//! let words = rseq.words()?;
//! loop {
//!     let cpu = t.cpu_id()?;
//!     let w = words.get(cpu)?;
//!     match t.compare_exchange(w, 0, 7) {
//!         Ok(_) | Err(Error::Miss(_)) => break,
//!         Err(Error::Abort) => {}
//!     }
//! }
//! # Some(())
//! # }
//! # let _ = try_it();
//! ```

mod abi;
mod attempt;
mod cpus;
mod membarrier;
mod region;
mod rseq;
mod thread;
mod words;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod aarch64;
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
mod fallback;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod registration;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod x86_64;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) use aarch64 as cs;
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub(crate) use fallback as cs;
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
pub(crate) use fallback as registration;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) use x86_64 as cs;

pub use rseq::{Available, Rseq};
pub use thread::{Cid, CpuId, Error, Index, NodeId, Thread};
pub use words::{Word, Words};
