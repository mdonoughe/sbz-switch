use std::ptr::null_mut;

use ole32::{CoInitializeEx, CoUninitialize};
use winapi::COINIT_APARTMENTTHREADED;

use hresult::{Win32Error, check};

pub fn initialize_com() -> Result<(), Win32Error> {
    unsafe {
        check(CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED)).and(Ok(()))
    }
}

pub fn uninitialize_com() {
    unsafe {
        CoUninitialize();
    }
}
