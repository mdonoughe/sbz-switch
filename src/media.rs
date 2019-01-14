//! Provides a Rust layer over the Windows IMMDevice API.

#![allow(unknown_lints)]

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::isize;
use std::mem;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr;
use std::slice;

use regex::Regex;
use slog::Logger;
use winapi::shared::guiddef::GUID;
use winapi::shared::winerror::NTE_NOT_FOUND;
use winapi::shared::wtypes::{PROPERTYKEY, VARTYPE};
use winapi::um::combaseapi::CLSCTX_ALL;
use winapi::um::combaseapi::{CoCreateInstance, CoTaskMemFree, PropVariantClear};
use winapi::um::coml2api::STGM_READ;
use winapi::um::endpointvolume::IAudioEndpointVolume;
use winapi::um::mmdeviceapi::{
    eConsole, eRender, CLSID_MMDeviceEnumerator, IMMDevice, IMMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use winapi::um::propidl::PROPVARIANT;
use winapi::um::propsys::IPropertyStore;
use winapi::Interface;

use crate::com::{ComObject, ComScope};
use crate::hresult::{check, Win32Error};
use crate::lazy::Lazy;
use crate::soundcore::{SoundCoreError, PKEY_SOUNDCORECTL_CLSID};
use crate::winapiext::{PKEY_DeviceInterface_FriendlyName, PKEY_Device_DeviceDesc};

fn parse_guid(src: &str) -> Result<GUID, Box<Error>> {
    let re1 = Regex::new(
        "^\\{([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-\
         ([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\\}$",
    )
    .unwrap();
    let re2 = Regex::new(
        "^([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-\
         ([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$",
    )
    .unwrap();
    let re3 = Regex::new(
        "^([0-9a-fA-F]{8})([0-9a-fA-F]{4})\
         ([0-9a-fA-F]{4})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$",
    )
    .unwrap();

    let caps = re1
        .captures(src)
        .or_else(|| re2.captures(src))
        .or_else(|| re3.captures(src))
        .ok_or(SoundCoreError::NotSupported)?;

    let mut iter = caps.iter().skip(1).map(|c| c.unwrap().as_str());
    let l = u32::from_str_radix(iter.next().unwrap(), 16).unwrap();
    let w1 = u16::from_str_radix(iter.next().unwrap(), 16).unwrap();
    let w2 = u16::from_str_radix(iter.next().unwrap(), 16).unwrap();
    let mut array = [0 as u8; 8];
    for b in iter.enumerate() {
        array[b.0] = u8::from_str_radix(b.1, 16).unwrap();
    }

    Ok(GUID {
        Data1: l,
        Data2: w1,
        Data3: w2,
        Data4: array,
    })
}

/// Represents an audio device.
pub struct Endpoint {
    device: ComObject<IMMDevice>,
    logger: Logger,
    volume: Lazy<Result<ComObject<IAudioEndpointVolume>, Win32Error>>,
    properties: Lazy<Result<PropertyStore, Win32Error>>,
}

impl Endpoint {
    fn new(device: ComObject<IMMDevice>, logger: Logger) -> Self {
        Self {
            device,
            logger,
            volume: Lazy::new(),
            properties: Lazy::new(),
        }
    }
    /// Gets the ID of the endpoint.
    ///
    /// See [Endpoint ID Strings](https://docs.microsoft.com/en-us/windows/desktop/CoreAudio/endpoint-id-strings).
    pub fn id(&self) -> Result<String, Win32Error> {
        unsafe {
            trace!(self.logger, "Getting device ID...");
            let mut raw_id = mem::uninitialized();
            check(self.device.GetId(&mut raw_id))?;
            let length = (0..isize::MAX)
                .position(|i| *raw_id.offset(i) == 0)
                .unwrap();
            let str: OsString = OsStringExt::from_wide(slice::from_raw_parts(raw_id, length));
            CoTaskMemFree(raw_id as *mut _);
            Ok(str.to_string_lossy().into_owned())
        }
    }
    fn property_store(&self) -> Result<&PropertyStore, Win32Error> {
        self.properties
            .get_or_create(|| unsafe {
                trace!(self.logger, "Opening PropertyStore...");
                let mut property_store = mem::uninitialized();
                check(self.device.OpenPropertyStore(
                    STGM_READ,
                    &mut property_store as *mut *mut IPropertyStore as *mut _,
                ))?;
                Ok(PropertyStore(
                    ComObject::take(property_store),
                    self.logger.clone(),
                ))
            })
            .as_ref()
            .map_err(|e| e.clone())
    }
    /// Gets the CLSID of the class implementing Creative's APIs.
    ///
    /// This allows discovery of a SoundCore implementation for devices that support it.
    pub fn clsid(&self) -> Result<GUID, SoundCoreError> {
        match self
            .property_store()?
            .get_string_value(&PKEY_SOUNDCORECTL_CLSID)
        {
            Ok(str) => parse_guid(&str).or(Err(SoundCoreError::NotSupported)),
            Err(GetPropertyError::UnexpectedType(_)) => Err(SoundCoreError::NotSupported),
            Err(GetPropertyError::Win32(ref error)) if error.code == NTE_NOT_FOUND => {
                Err(SoundCoreError::NotSupported)
            }
            Err(GetPropertyError::Win32(error)) => Err(SoundCoreError::Win32(error)),
        }
    }
    /// Gets the friendly name of the audio interface (sound adapter).
    ///
    /// See [Core Audio Properties: Device Properties](https://docs.microsoft.com/en-us/windows/desktop/coreaudio/core-audio-properties#device-properties).
    pub fn interface(&self) -> Result<String, GetPropertyError> {
        self.property_store()?
            .get_string_value(&PKEY_DeviceInterface_FriendlyName)
    }
    /// Gets a description of the audio endpoint (speakers, headphones, etc).
    ///
    /// See [Core Audio Properties: Device Properties](https://docs.microsoft.com/en-us/windows/desktop/coreaudio/core-audio-properties#device-properties).
    pub fn description(&self) -> Result<String, GetPropertyError> {
        self.property_store()?
            .get_string_value(&PKEY_Device_DeviceDesc)
    }
    fn volume(&self) -> Result<ComObject<IAudioEndpointVolume>, Win32Error> {
        self.volume
            .get_or_create(|| unsafe {
                let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
                check(self.device.Activate(
                    &IAudioEndpointVolume::uuidof(),
                    CLSCTX_ALL,
                    ptr::null_mut(),
                    &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _,
                ))?;
                Ok(ComObject::take(ctrl))
            })
            .clone()
    }
    /// Checks whether the device is already muted.
    pub fn get_mute(&self) -> Result<bool, Win32Error> {
        unsafe {
            trace!(self.logger, "Checking if we are muted...");
            let mut mute = 0;
            check(self.volume()?.GetMute(&mut mute))?;
            debug!(self.logger, "Muted = {}", mute);
            Ok(mute != 0)
        }
    }
    /// Mutes or unmutes the device.
    pub fn set_mute(&self, mute: bool) -> Result<(), Win32Error> {
        unsafe {
            let mute = if mute { 1 } else { 0 };
            info!(self.logger, "Setting muted to {}...", mute);
            check(self.volume()?.SetMute(mute, ptr::null_mut()))?;
            Ok(())
        }
    }
    /// Sets the volume of the device.
    ///
    /// Volumes range from 0.0 to 1.0.
    ///
    /// Volume can be controlled independent of muting.
    pub fn set_volume(&self, volume: f32) -> Result<(), Win32Error> {
        unsafe {
            info!(self.logger, "Setting volume to {}...", volume);
            check(
                self.volume()?
                    .SetMasterVolumeLevelScalar(volume, ptr::null_mut()),
            )?;
            Ok(())
        }
    }
    /// Gets the volume of the device.
    pub fn get_volume(&self) -> Result<f32, Win32Error> {
        unsafe {
            debug!(self.logger, "Getting volume...");
            let mut volume: f32 = mem::uninitialized();
            check(
                self.volume()?
                    .GetMasterVolumeLevelScalar(&mut volume as *mut f32),
            )?;
            debug!(self.logger, "volume = {}", volume);
            Ok(volume)
        }
    }
}

/// Describes an error that occurred while retrieving a property from a device.
#[derive(Debug)]
pub enum GetPropertyError {
    /// A Win32 error occurred.
    Win32(Win32Error),
    /// The returned value was not the expected type.
    UnexpectedType(VARTYPE),
}

impl fmt::Display for GetPropertyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GetPropertyError::Win32(error) => error.fmt(f),
            GetPropertyError::UnexpectedType(code) => {
                write!(f, "returned property value was of unexpected type {}", code)
            }
        }
    }
}

impl Error for GetPropertyError {
    fn cause(&self) -> Option<&Error> {
        match self {
            GetPropertyError::Win32(error) => Some(error),
            _ => None,
        }
    }
}

impl From<Win32Error> for GetPropertyError {
    fn from(error: Win32Error) -> Self {
        GetPropertyError::Win32(error)
    }
}

struct PropertyStore(ComObject<IPropertyStore>, Logger);

impl PropertyStore {
    unsafe fn get_value(&self, key: &PROPERTYKEY) -> Result<PROPVARIANT, Win32Error> {
        trace!(self.1, "Getting property...");
        let mut property_value = mem::uninitialized();
        check(self.0.GetValue(key, &mut property_value))?;
        Ok(property_value)
    }
    #[allow(clippy::cast_ptr_alignment)]
    fn get_string_value(&self, key: &PROPERTYKEY) -> Result<String, GetPropertyError> {
        unsafe {
            let mut property_value = self.get_value(key)?;
            trace!(self.1, "Returned variant has type {}", property_value.vt);
            // VT_LPWSTR
            if property_value.vt != 31 {
                PropVariantClear(&mut property_value);
                return Err(GetPropertyError::UnexpectedType(property_value.vt));
            }
            let chars: *const u16 =
                ptr::read_unaligned(property_value.data.as_ptr() as *const *const u16);
            let length = (0..isize::MAX).position(|i| *chars.offset(i) == 0);
            let str = length.map(|length| {
                OsString::from_wide(slice::from_raw_parts(chars, length))
                    .to_string_lossy()
                    .into_owned()
            });
            PropVariantClear(&mut property_value);
            let str = str.unwrap();
            trace!(self.1, "Returned variant has value {}", &str);
            Ok(str)
        }
    }
}

/// Provides access to the devices available in the current Windows session.
pub struct DeviceEnumerator(ComObject<IMMDeviceEnumerator>, Logger);

impl DeviceEnumerator {
    /// Creates a new device enumerator with the provided logger.
    pub fn with_logger(logger: Logger) -> Result<Self, Win32Error> {
        unsafe {
            let _scope = ComScope::begin();
            let mut enumerator: *mut IMMDeviceEnumerator = mem::uninitialized();
            trace!(logger, "Creating DeviceEnumerator...");
            check(CoCreateInstance(
                &CLSID_MMDeviceEnumerator,
                ptr::null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::uuidof(),
                &mut enumerator as *mut *mut IMMDeviceEnumerator as *mut _,
            ))?;
            trace!(logger, "Created DeviceEnumerator");
            Ok(DeviceEnumerator(ComObject::take(enumerator), logger))
        }
    }
    /// Gets all active audio outputs.
    #[allow(clippy::unnecessary_mut_passed)]
    pub fn get_active_audio_endpoints(&self) -> Result<Vec<Endpoint>, Win32Error> {
        unsafe {
            trace!(self.1, "Getting active endpoints...");
            let mut collection = mem::uninitialized();
            check(
                self.0
                    .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &mut collection),
            )?;
            let mut count = 0;
            check((*collection).GetCount(&mut count))?;
            let mut result = Vec::with_capacity(count as usize);
            for i in 0..count {
                let mut device = mem::uninitialized();
                check((*collection).Item(i, &mut device))?;
                result.push(Endpoint::new(ComObject::take(device), self.1.clone()))
            }
            Ok(result)
        }
    }
    /// Gets the default audio output.
    ///
    /// There are multiple default audio outputs in Windows.
    /// This function gets the device that would be used if the current application
    /// were to play music or sound effects (as opposed to VOIP audio).
    pub fn get_default_audio_endpoint(&self) -> Result<Endpoint, Win32Error> {
        unsafe {
            trace!(self.1, "Getting default endpoint...");
            let mut device = mem::uninitialized();
            check(
                self.0
                    .GetDefaultAudioEndpoint(eRender, eConsole, &mut device),
            )?;
            Ok(Endpoint::new(ComObject::take(device), self.1.clone()))
        }
    }
    /// Get a specific audio endpoint by its ID.
    pub fn get_endpoint(&self, id: &OsStr) -> Result<Endpoint, Win32Error> {
        trace!(self.1, "Getting endpoint...");
        let buffer: Vec<_> = id.encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut device = mem::uninitialized();
            check(self.0.GetDevice(buffer.as_ptr(), &mut device))?;
            Ok(Endpoint::new(ComObject::take(device), self.1.clone()))
        }
    }
}
