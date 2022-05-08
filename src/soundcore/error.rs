use std::error::Error;
use std::fmt;

use crate::media::GetPropertyError;

/// Describes an error that occurred while acting on Creative's SoundCore API.
#[derive(Debug)]
pub enum SoundCoreError {
    /// Some Win32 error occurred.
    Win32(windows::core::Error),
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

impl From<windows::core::Error> for SoundCoreError {
    fn from(err: windows::core::Error) -> SoundCoreError {
        SoundCoreError::Win32(err)
    }
}

impl From<GetPropertyError> for SoundCoreError {
    fn from(err: GetPropertyError) -> SoundCoreError {
        match err {
            GetPropertyError::UnexpectedType(_) => SoundCoreError::NotSupported,
            GetPropertyError::Win32(inner) => inner.into(),
        }
    }
}
