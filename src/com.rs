use std::ptr::null_mut;

use winapi::um::combaseapi::{CoInitializeEx, CoUninitialize};
use winapi::um::objbase::COINIT_APARTMENTTHREADED;

use hresult::{check, Win32Error};

/// Prepares the current thread for running COM by calling CoInitializeEx.
///
/// This initializes the current thread in single-threaded apartment mode (STA).
///
/// CoInitializeEx may be called multiple times.
///
/// # Examples
///
/// ```
/// initialize_com();
/// // do COM things
/// uninitialize_com();
/// ```
pub fn initialize_com() -> Result<(), Win32Error> {
    unsafe { check(CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED)).and(Ok(())) }
}

/// Unconfigures COM for the current thread by calling CoUninitialize.
///
/// This may not actually do anything if CoInitializeEx has been called multiple times.
pub fn uninitialize_com() {
    unsafe {
        CoUninitialize();
    }
}

pub(crate) struct ComScope {}

impl ComScope {
    pub fn new() -> Result<Self, Win32Error> {
        initialize_com()?;
        Ok(Self {})
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        uninitialize_com();
    }
}
