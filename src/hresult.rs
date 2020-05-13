use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;

use winapi::shared::ntdef::HRESULT;
use winapi::shared::winerror::FACILITY_WIN32;
use winapi::um::winbase::{
    FormatMessageW, LocalFree, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
};

/// Represents an error thrown from Win32 code.
#[derive(Clone, Debug)]
pub struct Win32Error {
    /// The original `HRESULT` value.
    pub code: HRESULT,
}

impl Win32Error {
    /// Creates a new `Win32Error` for an HRESULT value.
    ///
    /// # Examples
    ///
    /// ```
    /// let error = GetLastError();
    /// Win32Error::new(error)
    /// ```
    pub fn new(code: HRESULT) -> Win32Error {
        Win32Error { code }
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self.code >> 16) & 0x1fff {
            // if it's a win32 error, use FormatMessage to get a description
            facility if facility == FACILITY_WIN32 => unsafe {
                let mut buffer = MaybeUninit::<*mut u16>::uninit();
                let len = FormatMessageW(
                    FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM,
                    ptr::null(),
                    (self.code & 0xffff) as u32,
                    0,
                    buffer.as_mut_ptr() as *mut _,
                    0,
                    ptr::null_mut(),
                );
                match len {
                    0 => write!(f, "Unexpected HRESULT: {:X}", self.code),
                    len => {
                        let buffer = buffer.assume_init();
                        let str: OsString =
                            OsStringExt::from_wide(slice::from_raw_parts(buffer, len as usize));
                        LocalFree(buffer as *mut _);
                        write!(
                            f,
                            "Unexpected HRESULT: {:X}: {}",
                            self.code,
                            str.to_string_lossy()
                        )
                    }
                }
            },
            _ => write!(f, "Unexpected HRESULT: {:X}", self.code),
        }
    }
}

impl Error for Win32Error {
    fn cause(&self) -> Option<&dyn Error> {
        None
    }
}

/// Converts an `HRESULT` to a `Result<HRESULT, Win32Error>`.
///
/// `Ok(HRESULT)` can usually be ignored.
///
/// # Examples
///
/// ```
/// check(GetLastError())?
/// ```
#[inline]
pub fn check(result: HRESULT) -> Result<HRESULT, Win32Error> {
    match result {
        err if err < 0 => Err(Win32Error::new(err)),
        success => Ok(success),
    }
}
