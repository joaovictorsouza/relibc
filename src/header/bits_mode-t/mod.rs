#[cfg(any(target_os = "linux", target_os = "none"))]
use crate::platform::types::c_uint;

#[cfg(not(any(target_os = "linux", target_os = "none")))]
use crate::platform::types::c_int;

/// Used for some file attributes.
#[allow(non_camel_case_types)]
#[cfg(any(target_os = "linux", target_os = "none"))]
pub type mode_t = c_uint;
/// Used for some file attributes.
#[allow(non_camel_case_types)]
#[cfg(not(any(target_os = "linux", target_os = "none")))]
pub type mode_t = c_int;
