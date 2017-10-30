use std::error::Error;
use std::fmt;
use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use ole32::CoCreateInstance;
use slog::Logger;
use winapi::{CLSCTX_ALL, GUID};

use ctsndcr::{HardwareInfo, IID_SOUND_CORE, ISoundCore, Param, ParamValue};
use hresult::{Win32Error, check};

#[derive(Debug)]
pub enum SoundCoreError {
    Win32(Win32Error),
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
    fn description(&self) -> &str {
        match *self {
            SoundCoreError::Win32(ref err) => err.description(),
            SoundCoreError::NotSupported => "SoundCore not supported",
        }
    }
    fn cause(&self) -> Option<&Error> {
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

pub struct SoundCore<'a>(*mut ISoundCore, &'a Logger);

impl<'a> SoundCore<'a> {
    fn bind_hardware(&self, id: &str) {
        trace!(self.1, "Binding SoundCore to {}...", id);
        let mut buffer = [0; 260];
        for c in OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo {
            info_type: 0,
            info: buffer,
        };
        unsafe { (*self.0).BindHardware(&info) }
    }
    pub fn set_speakers(&self, code: u32) {
        info!(self.1, "Setting speaker configuration to {:x}...", code);
        unsafe {
            let param = Param {
                context: 0,
                feature: 0x1000002,
                param: 0,
            };
            let value = ParamValue {
                kind: 2,
                value: code,
            };
            (*self.0).SetParamValue(param, value)
        }
    }
}

impl<'a> Drop for SoundCore<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing SoundCore...");
            (*self.0).Release();
        }
    }
}

fn create_sound_core<'a>(
    clsid: &GUID,
    logger: &'a Logger,
) -> Result<SoundCore<'a>, SoundCoreError> {
    unsafe {
        let mut sc: *mut ISoundCore = mem::uninitialized();
        check(CoCreateInstance(
            clsid,
            ptr::null_mut(),
            CLSCTX_ALL,
            &IID_SOUND_CORE,
            &mut sc as *mut *mut ISoundCore as *mut _,
        ))?;
        Ok(SoundCore(sc, logger))
    }
}

pub fn get_sound_core<'a>(
    clsid: &GUID,
    id: &str,
    logger: &'a Logger,
) -> Result<SoundCore<'a>, SoundCoreError> {
    let core = create_sound_core(clsid, logger)?;
    core.bind_hardware(id);
    Ok(core)
}
