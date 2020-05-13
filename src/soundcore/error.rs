use std::error::Error;
use std::fmt;

use crate::hresult::Win32Error;

/// Describes an error that occurred while acting on Creative's SoundCore API.
#[derive(Debug)]
pub enum SoundCoreError {
    /// Some Win32 error occurred.
    Win32(Win32Error),
    /// The specified device does not support implement the SoundCore API.
    NotSupported,
}

impl fmt::Display for SoundCoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SoundCoreError::Win32(ref err) => write!(f, "Win32Error: {}", err),
            SoundCoreError::NotSupported => write!(f, "SoundCore not supported"),
        }
    }
}

impl Error for SoundCoreError {
    fn cause(&self) -> Option<&dyn Error> {
        match *self {
            SoundCoreError::Win32(ref err) => Some(err),
            SoundCoreError::NotSupported => None,
        }
    }
}

impl From<Win32Error> for SoundCoreError {
    fn from(err: Win32Error) -> SoundCoreError {
        SoundCoreError::Win32(err)
    }
}
