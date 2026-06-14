pub use self::sys::*;

#[cfg(any(target_os = "linux", target_os = "none"))]
#[path = "linux.rs"]
pub mod sys;

#[cfg(target_os = "redox")]
#[path = "redox.rs"]
pub mod sys;
