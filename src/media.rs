use std::error::Error;
use std::ffi::OsString;
use std::isize;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr::{self, NonNull};
use std::slice;

use regex::Regex;
use slog::Logger;
use winapi::shared::guiddef::GUID;
use winapi::shared::winerror::NTE_NOT_FOUND;
use winapi::shared::wtypes::PROPERTYKEY;
use winapi::um::combaseapi::CLSCTX_ALL;
use winapi::um::combaseapi::{CoCreateInstance, CoTaskMemFree};
use winapi::um::coml2api::STGM_READ;
use winapi::um::endpointvolume::IAudioEndpointVolume;
use winapi::um::mmdeviceapi::{
    eConsole, eRender, CLSID_MMDeviceEnumerator, IMMDevice, IMMDeviceEnumerator,
};
use winapi::um::propidl::PROPVARIANT;
use winapi::um::propsys::IPropertyStore;
use winapi::Interface;

use hresult::{check, Win32Error};
use lazy::Lazy;
use soundcore::{SoundCoreError, PKEY_SOUNDCORECTL_CLSID};

fn get_device_enumerator(logger: Logger) -> Result<DeviceEnumerator, Win32Error> {
    unsafe {
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
        Ok(DeviceEnumerator(NonNull::new(enumerator).unwrap(), logger))
    }
}

fn parse_guid(src: &str) -> Result<GUID, Box<Error>> {
    let re1 = Regex::new(
        "^\\{([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-\
         ([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\\}$",
    ).unwrap();
    let re2 = Regex::new(
        "^([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-\
         ([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$",
    ).unwrap();
    let re3 = Regex::new(
        "^([0-9a-fA-F]{8})([0-9a-fA-F]{4})\
         ([0-9a-fA-F]{4})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\
         ([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$",
    ).unwrap();

    let caps = re1.captures(src)
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

pub struct Endpoint {
    device: NonNull<IMMDevice>,
    logger: Logger,
    volume: Lazy<Result<NonNull<IAudioEndpointVolume>, Win32Error>>,
    properties: Lazy<Result<PropertyStore, Win32Error>>,
}

impl Drop for Endpoint {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.logger, "Releasing Device...");
            self.device.as_mut().Release();
            if let Some(Ok(mut volume)) = self.volume.get() {
                volume.as_mut().Release();
            }
        }
    }
}

impl Endpoint {
    fn new(device: NonNull<IMMDevice>, logger: Logger) -> Self {
        Self {
            device,
            logger,
            volume: Lazy::new(),
            properties: Lazy::new(),
        }
    }
    pub fn id(&self) -> Result<String, Win32Error> {
        unsafe {
            trace!(self.logger, "Getting device ID...");
            let mut raw_id = mem::uninitialized();
            check(self.device.as_ref().GetId(&mut raw_id))?;
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
                check(self.device.as_ref().OpenPropertyStore(
                    STGM_READ,
                    &mut property_store as *mut *mut IPropertyStore as *mut _,
                ))?;
                Ok(PropertyStore(
                    NonNull::new(property_store).unwrap(),
                    self.logger.clone(),
                ))
            })
            .as_ref()
            .map_err(|e| e.clone())
    }
    pub fn clsid(&self) -> Result<GUID, SoundCoreError> {
        #[allow(unknown_lints, unreadable_literal)]
        unsafe {
            let property_result = self.property_store()?.get_value(&PKEY_SOUNDCORECTL_CLSID);
            match property_result {
                Err(ref err) if err.code == NTE_NOT_FOUND => {
                    return Err(SoundCoreError::NotSupported)
                }
                Err(err) => return Err(SoundCoreError::Win32(err)),
                Ok(_) => {}
            }
            let property_value = property_result?;
            trace!(
                self.logger,
                "Returned variant has type {}",
                property_value.vt
            );
            // VT_LPWSTR
            if property_value.vt != 31 {
                return Err(SoundCoreError::NotSupported);
            }
            let chars = *(property_value.data.as_ptr() as *mut *mut u16);
            let length = (0..isize::MAX).position(|i| *chars.offset(i) == 0).unwrap();
            let str = OsString::from_wide(slice::from_raw_parts(chars, length))
                .to_string_lossy()
                .into_owned();
            trace!(self.logger, "Returned variant has value {}", &str);
            parse_guid(&str).or(Err(SoundCoreError::NotSupported))
        }
    }
    fn volume(&self) -> Result<NonNull<IAudioEndpointVolume>, Win32Error> {
        self.volume
            .get_or_create(|| unsafe {
                let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
                check(self.device.as_ref().Activate(
                    &IAudioEndpointVolume::uuidof(),
                    CLSCTX_ALL,
                    ptr::null_mut(),
                    &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _,
                ))?;
                Ok(NonNull::new(ctrl).unwrap())
            })
            .clone()
    }
    pub fn get_mute(&self) -> Result<bool, Win32Error> {
        unsafe {
            trace!(self.logger, "Checking if we are muted...");
            let mut mute = 0;
            check(self.volume()?.as_ref().GetMute(&mut mute))?;
            debug!(self.logger, "Muted = {}", mute);
            Ok(mute != 0)
        }
    }
    pub fn set_mute(&self, mute: bool) -> Result<(), Win32Error> {
        unsafe {
            let mute = if mute { 1 } else { 0 };
            info!(self.logger, "Setting muted to {}...", mute);
            check(self.volume()?.as_mut().SetMute(mute, ptr::null_mut()))?;
            Ok(())
        }
    }
    pub fn set_volume(&self, volume: f32) -> Result<(), Win32Error> {
        unsafe {
            info!(self.logger, "Setting volume to {}...", volume);
            check(
                self.volume()?
                    .as_mut()
                    .SetMasterVolumeLevelScalar(volume, ptr::null_mut()),
            )?;
            Ok(())
        }
    }
    pub fn get_volume(&self) -> Result<f32, Win32Error> {
        unsafe {
            debug!(self.logger, "Getting volume...");
            let mut volume: f32 = mem::uninitialized();
            check(
                self.volume()?
                    .as_ref()
                    .GetMasterVolumeLevelScalar(&mut volume as *mut f32),
            )?;
            debug!(self.logger, "volume = {}", volume);
            Ok(volume)
        }
    }
}

struct PropertyStore(NonNull<IPropertyStore>, Logger);

impl Drop for PropertyStore {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing PropertyStore...");
            self.0.as_mut().Release();
        }
    }
}

impl PropertyStore {
    fn get_value(&self, key: &PROPERTYKEY) -> Result<PROPVARIANT, Win32Error> {
        unsafe {
            let mut property_value = mem::uninitialized();
            check(self.0.as_ref().GetValue(key, &mut property_value))?;
            Ok(property_value)
        }
    }
}

pub fn get_default_endpoint(logger: Logger) -> Result<Endpoint, Win32Error> {
    get_device_enumerator(logger)?.get_default_audio_endpoint()
}

struct DeviceEnumerator(NonNull<IMMDeviceEnumerator>, Logger);

impl DeviceEnumerator {
    fn get_default_audio_endpoint(&self) -> Result<Endpoint, Win32Error> {
        unsafe {
            trace!(self.1, "Getting default endpoint...");
            let mut device = mem::uninitialized();
            check(
                self.0
                    .as_ref()
                    .GetDefaultAudioEndpoint(eRender, eConsole, &mut device),
            )?;
            Ok(Endpoint::new(NonNull::new(device).unwrap(), self.1.clone()))
        }
    }
}

impl Drop for DeviceEnumerator {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing DeviceEnumerator...");
            self.0.as_mut().Release();
        }
    }
}
