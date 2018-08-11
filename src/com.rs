use std::ptr::null_mut;

use winapi::um::combaseapi::{CoInitializeEx, CoUninitialize};
use winapi::um::objbase::COINIT_APARTMENTTHREADED;

use hresult::{check, Win32Error};

pub fn initialize_com() -> Result<(), Win32Error> {
    unsafe { check(CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED)).and(Ok(())) }
}

pub fn uninitialize_com() {
    unsafe {
        CoUninitialize();
    }
}
