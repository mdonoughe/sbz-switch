use std::error::Error;
use std::fmt;
use winapi::shared::ntdef::HRESULT;

#[derive(Clone, Debug)]
pub struct Win32Error {
    pub code: HRESULT,
    description: String,
}

impl Win32Error {
    pub fn new(code: HRESULT) -> Win32Error {
        Win32Error {
            code: code,
            description: format!("{:08x}", code),
        }
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unexpected HRESULT: {}", self.code)
    }
}

impl Error for Win32Error {
    fn description(&self) -> &str {
        &self.description
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}

#[inline]
pub fn check(result: HRESULT) -> Result<HRESULT, Win32Error> {
    match result {
        err if err < 0 => Err(Win32Error::new(err)),
        success => Ok(success),
    }
}
