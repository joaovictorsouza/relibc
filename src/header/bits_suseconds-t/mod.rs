#[cfg(any(target_os = "linux", target_os = "none"))]
use crate::platform::types::c_long;

#[cfg(not(any(target_os = "linux", target_os = "none")))]
use crate::platform::types::c_int;

#[cfg(any(target_os = "linux", target_os = "none"))]
#[allow(non_camel_case_types)]
/// Used for time in microseconds.
pub type suseconds_t = c_long;
#[cfg(not(any(target_os = "linux", target_os = "none")))]
#[allow(non_camel_case_types)]
/// Used for time in microseconds.
pub type suseconds_t = c_int;
