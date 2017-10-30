use std::error::Error;
use std::ffi::OsString;
use std::isize;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;

use ole32::{CoCreateInstance, CoTaskMemFree};
use regex::Regex;
use slog::Logger;
use winapi::{CLSCTX_ALL, CLSID_MMDeviceEnumerator, eConsole, eRender, GUID, IID_IMMDeviceEnumerator, IMMDevice, IMMDeviceEnumerator, NTE_NOT_FOUND};

use hresult::{Win32Error, check};
use soundcore::SoundCoreError;
use winapiext::{IAudioEndpointVolume, IID_AUDIO_ENDPOINT_VOLUME, IPropertyStore, PROPERTYKEY, PROPVARIANT, STGM_READ};

fn get_device_enumerator<'a>(logger: &'a Logger) -> Result<DeviceEnumerator<'a>, Win32Error> {
    unsafe {
        let mut enumerator: *mut IMMDeviceEnumerator = mem::uninitialized();
        trace!(logger, "Creating DeviceEnumerator...");
        check(CoCreateInstance(&CLSID_MMDeviceEnumerator,
              ptr::null_mut(), CLSCTX_ALL,
              &IID_IMMDeviceEnumerator,
              &mut enumerator as *mut *mut IMMDeviceEnumerator as *mut _))?;
        trace!(logger, "Created DeviceEnumerator");
        Ok(DeviceEnumerator(enumerator, logger))
    }
}

fn parse_guid(src: &str) -> Result<GUID, Box<Error>> {
    let re1 = Regex::new(r"^\{([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\}$").unwrap();
    let re2 = Regex::new(r"^([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap();
    let re3 = Regex::new(r"^([0-9a-fA-F]{8})([0-9a-fA-F]{4})([0-9a-fA-F]{4})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap();

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
        Data4: array
    })
}

pub struct Endpoint<'a>(*mut IMMDevice, *mut IAudioEndpointVolume, &'a Logger);

impl<'a> Drop for Endpoint<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.2, "Releasing Device...");
            (*self.0).Release();
            (*self.1).Release();
        }
    }
}

impl<'a> Endpoint<'a> {
    pub fn id(&self) -> Result<String, Win32Error> {
        unsafe {
            trace!(self.2, "Getting device ID...");
            let mut raw_id = mem::uninitialized();
            check((*self.0).GetId(&mut raw_id))?;
            let length = (0..isize::MAX).position(|i| *raw_id.offset(i) == 0).unwrap();
            let str: OsString = OsStringExt::from_wide(slice::from_raw_parts(raw_id, length));
            CoTaskMemFree(raw_id as *mut _);
            Ok(str.to_string_lossy().into_owned())
        }
    }
    fn property_store(&self) -> Result<PropertyStore<'a>, Win32Error> {
        unsafe {
            trace!(self.2, "Opening PropertyStore...");
            let mut property_store = mem::uninitialized();
            check((*self.0).OpenPropertyStore(STGM_READ, &mut property_store as *mut *mut IPropertyStore as *mut _))?;
            Ok(PropertyStore(property_store, self.2))
        }
    }
    pub fn clsid(&self) -> Result<GUID, SoundCoreError> {
        const KEY_SOUNDCORECTL_CLSID : PROPERTYKEY = PROPERTYKEY {
            fmtid: GUID {
                Data1: 0xc949c6aa,
                Data2: 0x132b,
                Data3: 0x4511,
                Data4: [0xbb, 0x1b, 0x35, 0x26, 0x1a, 0x2a, 0x63, 0x33]
            },
            pid: 0
        };
        unsafe {
            let property_result = self.property_store()?.get_value(KEY_SOUNDCORECTL_CLSID);
            match property_result {
                Err(ref err) if err.code == NTE_NOT_FOUND => return Err(SoundCoreError::NotSupported),
                Err(err) => return Err(SoundCoreError::Win32(err)),
                Ok(_) => {}
            }
            let property_value = property_result?;
            trace!(self.2, "Returned variant has type {}", property_value.vt);
            // VT_LPWSTR
            if property_value.vt != 31 {
                return Err(SoundCoreError::NotSupported)
            }
            let chars = *(property_value.data.as_ptr() as *mut *mut u16);
            let length = (0..isize::MAX).position(|i| *chars.offset(i) == 0).unwrap();
            let str = OsString::from_wide(slice::from_raw_parts(chars, length)).to_string_lossy().into_owned();
            trace!(self.2, "Returned variant has value {}", &str);
            parse_guid(&str).or(Err(SoundCoreError::NotSupported))
        }
    }
    pub fn get_mute(&self) -> Result<bool, Win32Error> {
        unsafe {
            trace!(self.2, "Checking if we are muted...");
            let mut mute = false;
            check((*self.1).GetMute(&mut mute))?;
            debug!(self.2, "Muted = {}", mute);
            Ok(mute)
        }
    }
    pub fn set_mute(&self, mute: bool) -> Result<(), Win32Error> {
        unsafe {
            info!(self.2, "Setting muted to {}...", mute);
            check((*self.1).SetMute(mute, ptr::null_mut()))?;
            Ok(())
        }
    }
    pub fn set_volume(&self, volume: f32) -> Result<(), Win32Error> {
        unsafe {
            info!(self.2, "Setting volume to {}...", volume);
            check((*self.1).SetMasterVolumeLevelScalar(volume, ptr::null_mut()))?;
            Ok(())
        }
    }
}

struct PropertyStore<'a>(*mut IPropertyStore, &'a Logger);

impl<'a> Drop for PropertyStore<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing PropertyStore...");
            (*self.0).Release();
        }
    }
}

impl<'a> PropertyStore<'a> {
    fn get_value(&self, key: PROPERTYKEY) -> Result<PROPVARIANT, Win32Error> {
        unsafe {
            let mut property_value = mem::uninitialized();
            check((*self.0).GetValue(&key, &mut property_value))?;
            Ok(property_value)
        }
    }
}

pub fn get_default_endpoint<'a>(logger: &'a Logger) -> Result<Endpoint<'a>, Win32Error> {
    get_device_enumerator(logger)?.get_default_audio_endpoint()
}

struct DeviceEnumerator<'a>(*mut IMMDeviceEnumerator, &'a Logger);

impl<'a> DeviceEnumerator<'a> {
    fn get_default_audio_endpoint(&self) -> Result<Endpoint<'a>, Win32Error> {
        unsafe {
            trace!(self.1, "Getting default endpoint...");
            let mut device = mem::uninitialized();
            check((*self.0).GetDefaultAudioEndpoint(eRender, eConsole, &mut device))?;
            let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
            trace!(self.1, "Getting volume control...");
            let volume = check((*device).Activate(&IID_AUDIO_ENDPOINT_VOLUME, CLSCTX_ALL, ptr::null_mut(), &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _));
            match volume {
                Ok(_) => Ok(Endpoint(device, ctrl, self.1)),
                Err(err) => {
                    error!(self.1, "Could not get volume control!");
                    (*device).Release();
                    Err(err)
                }
            }
        }
    }
}

impl<'a> Drop for DeviceEnumerator<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.1, "Releasing DeviceEnumerator...");
            (*self.0).Release();
        }
    }
}

