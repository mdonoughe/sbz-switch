extern crate clap;
extern crate ole32;
extern crate regex;
#[macro_use]
extern crate slog;
extern crate sloggers;
#[macro_use]
extern crate winapi;

use std::error;
use std::ffi;
use std::fmt;
use std::mem;
use std::ptr;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::ffi::OsStrExt;
use std::str::FromStr;

use clap::{Arg, App};

use regex::Regex;

use slog::Logger;

use sloggers::Build;
use sloggers::terminal::{TerminalLoggerBuilder, Destination};
use sloggers::types::Severity;

mod winapiext;

#[derive(Debug)]
pub struct Win32Error {
    pub code: winapi::HRESULT,
    description: String,
}

impl Win32Error {
    pub fn new(code: winapi::HRESULT) -> Win32Error {
        Win32Error { code: code, description: format!("{:08x}", code) }
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Unexpected HRESULT: {}", self.code)
    }
}

impl error::Error for Win32Error {
    fn description(&self) -> &str {
        &self.description
    }

    fn cause(&self) -> Option<&error::Error> {
        None
    }
}

#[derive(Debug)]
enum SoundCoreError {
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

impl error::Error for SoundCoreError {
    fn description(&self) -> &str {
        match *self {
            SoundCoreError::Win32(ref err) => err.description(),
            SoundCoreError::NotSupported => "SoundCore not supported",
        }
    }
    fn cause(&self) -> Option<&error::Error> {
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

#[inline]
fn check(result: winapi::HRESULT) -> Result<winapi::HRESULT, Win32Error> {
    match result {
        err if err < 0 => Err(Win32Error::new(err)),
        success => Ok(success)
    }
}

fn get_device_enumerator<'a>(logger: &'a Logger) -> Result<DeviceEnumerator<'a>, Win32Error> {
    unsafe {
        let mut enumerator: *mut winapi::IMMDeviceEnumerator = mem::uninitialized();
        trace!(logger, "Creating DeviceEnumerator...");
        check(ole32::CoCreateInstance(&winapi::CLSID_MMDeviceEnumerator,
                                      ptr::null_mut(), winapi::CLSCTX_ALL,
                                      &winapi::IID_IMMDeviceEnumerator,
                                      &mut enumerator
                                               as *mut *mut winapi::IMMDeviceEnumerator
                                               as *mut _))?;
        trace!(logger, "Created DeviceEnumerator");
        Ok(DeviceEnumerator(enumerator, logger))
    }
}

struct Endpoint<'a>(*mut winapi::IMMDevice, &'a Logger);

impl<'a> Drop for Endpoint<'a> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.0).Release();
        }
    }
}

fn parse_guid(src: &str) -> Result<winapi::GUID, Box<error::Error>> {
    let re1 = Regex::new(r"^\{([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})\}$").unwrap();
    let re2 = Regex::new(r"^([0-9a-fA-F]{8})-([0-9a-fA-F]{4})-([0-9a-fA-F]{4})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})-([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap();
    let re3 = Regex::new(r"^([0-9a-fA-F]{8})([0-9a-fA-F]{4})([0-9a-fA-F]{4})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$").unwrap();

    let caps = re1.captures(src)
        .or_else(|| re2.captures(src))
        .or_else(|| re3.captures(src))
        .ok_or(SoundCoreError::NotSupported)?;

    let mut iter = caps.iter().skip(1).map(|c| c.unwrap().as_str());
    let l = u32::from_str_radix(iter.next().unwrap(), 16)?;
    let w1 = u16::from_str_radix(iter.next().unwrap(), 16)?;
    let w2 = u16::from_str_radix(iter.next().unwrap(), 16)?;
    let mut array = [0 as u8; 8];
    for b in iter.enumerate() {
        array[b.0] = u8::from_str_radix(b.1, 16)?;
    }

    Ok(winapi::GUID {
        Data1: l,
        Data2: w1,
        Data3: w2,
        Data4: array
    })
}

const IID_AUDIO_ENDPOINT_VOLUME: winapi::GUID = winapi::GUID {
    Data1: 0x5cdf2c82,
    Data2: 0x841e,
    Data3: 0x4546,
    Data4: [0x97, 0x22, 0x0c, 0xf7, 0x40, 0x78, 0x22, 0x9a]
};

impl<'a> Endpoint<'a> {
    fn id(&self) -> Result<String, Win32Error> {
        unsafe {
            trace!(self.1, "Getting device ID...");
            let mut raw_id = mem::uninitialized();
            check((*self.0).GetId(&mut raw_id))?;
            let length = (0..std::isize::MAX).position(|i| *raw_id.offset(i) == 0).unwrap();
            let str: ffi::OsString = OsStringExt::from_wide(std::slice::from_raw_parts(raw_id, length));
            ole32::CoTaskMemFree(raw_id as *mut _);
            Ok(str.to_string_lossy().into_owned())
        }
    }
    fn property_store(&self) -> Result<PropertyStore<'a>, Win32Error> {
        unsafe {
            trace!(self.1, "Opening PropertyStore...");
            let mut property_store = mem::uninitialized();
            check((*self.0).OpenPropertyStore(winapiext::STGM_READ, &mut property_store as *mut *mut winapiext::IPropertyStore as *mut _))?;
            Ok(PropertyStore(property_store, self.1))
        }
    }
    fn clsid(&self) -> Result<winapi::GUID, SoundCoreError> {
        const KEY_SOUNDCORECTL_CLSID : winapiext::PROPERTYKEY = winapiext::PROPERTYKEY {
            fmtid: winapi::GUID {
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
                Err(ref err) if err.code == winapi::NTE_NOT_FOUND => return Err(SoundCoreError::NotSupported),
                Err(err) => return Err(SoundCoreError::Win32(err)),
                Ok(_) => {}
            }
            let property_value = property_result?;
            trace!(self.1, "Returned variant has type {}", property_value.vt);
            // VT_LPWSTR
            if property_value.vt != 31 {
                return Err(SoundCoreError::NotSupported)
            }
            let chars = *(property_value.data.as_ptr() as *mut *mut u16);
            let length = (0..std::isize::MAX).position(|i| *chars.offset(i) == 0).unwrap();
            let str = ffi::OsString::from_wide(std::slice::from_raw_parts(chars, length)).to_string_lossy().into_owned();
            trace!(self.1, "Returned variant has value {}", &str);
            parse_guid(&str).or(Err(SoundCoreError::NotSupported))
        }
    }
    fn get_mute(&self) -> Result<bool, Win32Error> {
        unsafe {
            let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
            trace!(self.1, "Getting volume control...");
            check((*self.0).Activate(&IID_AUDIO_ENDPOINT_VOLUME, winapi::CLSCTX_ALL, ptr::null_mut(), &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _))?;
            trace!(self.1, "Checking if we are muted...");
            let mut mute = false;
            let result = check((*ctrl).GetMute(&mut mute));
            (*ctrl).Release();
            result?;
            debug!(self.1, "Muted = {}", mute);
            Ok(mute)
        }
    }
    fn set_mute(&self, mute: bool) -> Result<(), Win32Error> {
        unsafe {
            let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
            trace!(self.1, "Getting volume control...");
            check((*self.0).Activate(&IID_AUDIO_ENDPOINT_VOLUME, winapi::CLSCTX_ALL, ptr::null_mut(), &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _))?;
            info!(self.1, "Setting muted to {}...", mute);
            let result = check((*ctrl).SetMute(mute, ptr::null_mut()));
            (*ctrl).Release();
            result?;
            Ok(())
        }
    }
    fn set_volume(&self, volume: f32) -> Result<(), Win32Error> {
        unsafe {
            let mut ctrl: *mut IAudioEndpointVolume = mem::uninitialized();
            trace!(self.1, "Getting volume control...");
            check((*self.0).Activate(&IID_AUDIO_ENDPOINT_VOLUME, winapi::CLSCTX_ALL, ptr::null_mut(), &mut ctrl as *mut *mut IAudioEndpointVolume as *mut _))?;
            info!(self.1, "Setting volume to {}...", volume);
            let result = check((*ctrl).SetMasterVolumeLevelScalar(volume, ptr::null_mut()));
            (*ctrl).Release();
            result?;
            Ok(())
        }
    }
}

struct PropertyStore<'a>(*mut winapiext::IPropertyStore, &'a Logger);

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
    fn get_value(&self, key: winapiext::PROPERTYKEY) -> Result<winapiext::PROPVARIANT, Win32Error> {
        unsafe {
            let mut property_value = mem::uninitialized();
            check((*self.0).GetValue(&key, &mut property_value))?;
            Ok(property_value)
        }
    }
}

fn get_default_endpoint<'a>(logger: &'a Logger) -> Result<Endpoint<'a>, Win32Error> {
    get_device_enumerator(logger)?.get_default_audio_endpoint()
}

struct DeviceEnumerator<'a>(*mut winapi::IMMDeviceEnumerator, &'a Logger);

impl<'a> DeviceEnumerator<'a> {
    fn get_default_audio_endpoint(&self) -> Result<Endpoint<'a>, Win32Error> {
        unsafe {
            trace!(self.1, "Getting default endpoint...");
            let mut device = mem::uninitialized();
            check((*self.0).GetDefaultAudioEndpoint(winapi::eRender, winapi::eConsole, &mut device))?;
            Ok(Endpoint(device, self.1))
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

struct SoundCore<'a>(*mut ISoundCore, &'a Logger);

impl<'a> SoundCore<'a> {
    fn bind_hardware(&self, id: &str) {
        trace!(self.1, "Binding SoundCore to {}...", id);
        let mut buffer = [0; 260];
        for c in ffi::OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo { info_type: 0, info: buffer };
        unsafe {
            (*self.0).BindHardware(&info)
        }
    }
    fn set_speakers(&self, code: u32) {
        info!(self.1, "Setting speaker configuration to {:x}...", code);
        unsafe {
            let param = Param {
                context: 0,
                feature: 0x1000002,
                param: 0
            };
            let value = ParamValue {
                kind: 2,
                value: code
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

#[repr(C)]
pub struct HardwareInfo {
    pub info_type: winapi::DWORD,
    pub info: [u16; 260],
}

RIDL!(
interface ISoundCore(ISoundCoreVtbl): IUnknown(IUnknownVtbl) {
    fn BindHardware(
        &mut self,
        hardware_info: *const HardwareInfo
    ) -> (),
    fn EnumContexts(
        &mut self,
        index: u32,
        context_info: *mut ()
    ) -> (),
    fn GetContextInfo(
        &mut self,
        context: u32,
        context_info: *mut ()
    ) -> (),
    fn GetContext(
        &mut self,
        context: *mut u32
    ) -> (),
    fn SetContext(
        &mut self,
        context: u32,
        restore_state: u32
    ) -> (),
    fn EnumFeatures(
        &mut self,
        context: u32,
        index: u32,
        feature_info: *mut ()
    ) -> (),
    fn GetFeatureInfo(
        &mut self,
        context: u32,
        feature: u32,
        feature_info: *mut ()
    ) -> (),
    fn EnumParams(
        &mut self,
        context: u32,
        index: u32,
        feature: u32,
        param_info: *mut ()
    ) -> (),
    fn GetParamInfo(
        &mut self,
        param: Param,
        info: *mut ()
    ) -> (),
    fn GetParamValue(
        &mut self,
        param: Param,
        value: *mut ParamValue
    ) -> (),
    fn SetParamValue(
        &mut self,
        param: Param,
        value: ParamValue
    ) -> ()
}
);

#[repr(C)]
pub struct Param {
    context: u32,
    feature: u32,
    param: u32,
}

#[repr(C)]
pub struct ParamValue {
    kind: u32,
    value: u32,
}

RIDL!(
interface IAudioEndpointVolume(IAudioEndpointVolumeVtbl): IUnknown(IUnknownVtbl) {
    fn RegisterControlChangeNotify(
        &mut self,
        notify: *const ()
    ) -> winapi::HRESULT,
    fn UnregisterControlChangeNotify(
        &mut self,
        notify: *const ()
    ) -> winapi::HRESULT,
    fn GetChannelCount(
        &mut self,
        channel_count: *mut u32
    ) -> winapi::HRESULT,
    fn SetMasterVolumeLevel(
        &mut self,
        level_db: f32,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn SetMasterVolumeLevelScalar(
        &mut self,
        level: f32,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn GetMasterVolumeLevel(
        &mut self,
        level_db: *mut f32
    ) -> winapi::HRESULT,
    fn GetMasterVolumeLevelScalar(
        &mut self,
        level: *mut f32
    ) -> winapi::HRESULT,
    fn SetChannelVolumeLevel(
        &mut self,
        channel: u32,
        level_db: f32,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn SetChannelVolumeLevelScalar(
        &mut self,
        channel: u32,
        level: f32,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn GetChannelVolumeLevel(
        &mut self,
        channel: u32,
        level_db: *mut f32
    ) -> winapi::HRESULT,
    fn GetChannelVolumeLevelScalar(
        &mut self,
        channel: u32,
        level: *mut f32
    ) -> winapi::HRESULT,
    fn SetMute(
        &mut self,
        mute: bool,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn GetMute(
        &mut self,
        mute: *mut bool
    ) -> winapi::HRESULT,
    fn GetVolumeStepInfo(
        &mut self,
        step: *mut u32,
        step_count: *mut u32
    ) -> winapi::HRESULT,
    fn VolumeStepUp(
        &mut self,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn VolumeStepDown(
        &mut self,
        event_context: *const winapi::GUID
    ) -> winapi::HRESULT,
    fn QueryHardwareSupport(
        &mut self,
        hardware_support_mask: *mut winapi::DWORD
    ) -> winapi::HRESULT,
    fn GetVolumeRange(
        &mut self,
        level_min_db: *mut f32,
        level_max_db: *mut f32,
        volume_increment_db: *mut f32
    ) -> winapi::HRESULT
}
);

fn create_sound_core<'a>(clsid: &winapi::GUID, logger: &'a Logger) -> Result<SoundCore<'a>, SoundCoreError> {
    const IID_SOUND_CORE: winapi::GUID = winapi::GUID {
        Data1: 0x6111e7c4,
        Data2: 0x3ea4,
        Data3: 0x47ed,
        Data4: [0xb0, 0x74, 0xc6, 0x38, 0x87, 0x52, 0x82, 0xc4]
    };
    unsafe {
        let mut sc: *mut ISoundCore = mem::uninitialized();
        check(ole32::CoCreateInstance(clsid,
                                      ptr::null_mut(), winapi::CLSCTX_ALL,
                                      &IID_SOUND_CORE,
                                      &mut sc
                                               as *mut *mut ISoundCore
                                               as *mut _))?;
        Ok(SoundCore(sc, logger))
    }
}

fn get_sound_core<'a>(clsid: &winapi::GUID, id: &str, logger: &'a Logger) -> Result<SoundCore<'a>, SoundCoreError> {
    let core = create_sound_core(clsid, logger)?;
    core.bind_hardware(id);
    Ok(core)
}

fn go(logger: &Logger, speakers: u32, volume: Option<f32>) -> Result<(), Box<error::Error>> {
    let endpoint = get_default_endpoint(logger)?;

    let id = endpoint.id()?;
    debug!(logger, "Found device {}", id);
    let clsid = endpoint.clsid()?;
    debug!(logger, "Found clsid {{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}", clsid.Data1, clsid.Data2, clsid.Data3, clsid.Data4[0], clsid.Data4[1], clsid.Data4[2], clsid.Data4[3], clsid.Data4[4], clsid.Data4[5], clsid.Data4[6], clsid.Data4[7]);
    let core = get_sound_core(&clsid, &id, &logger)?;

    let premuted = endpoint.get_mute()?;
    if !premuted {
        endpoint.set_mute(true)?;
    }
    core.set_speakers(speakers);
    if volume.is_some() {
        endpoint.set_volume(volume.unwrap())?;
    }
    if !premuted {
        endpoint.set_mute(false)?;
    }

    Ok(())
}

fn main() {
    let matches = App::new("sbz-switch")
        .version("0.1")
        .about("Switches outputs on Creative Sound Blaster devices")
        .author("Matthew Donoughe <mdonoughe@gmail.com>")
        .arg(Arg::with_name("speakers")
            .short("s")
            .long("speakers")
            .value_name("CONFIG")
            .help("The speaker configuration to use. \"3003\" for stereo speakers, \"80000000\" for headphones.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("volume")
            .short("v")
            .long("volume")
            .value_name("VOLUME")
            .help("The target volume level, in percent.")
            .takes_value(true))
        .get_matches();

    let speakers = u32::from_str_radix(matches.value_of("speakers").unwrap(), 16).unwrap();
    let volume = matches.value_of("volume").map(|s| f32::from_str(s).unwrap() / 100.0);

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();
    unsafe {
        trace!(logger, "Initializing COM...");
        check(ole32::CoInitializeEx(ptr::null_mut(), winapi::COINIT_APARTMENTTHREADED)).unwrap();
        trace!(logger, "Initialized");
        let result = go(&logger, speakers, volume);
        trace!(logger, "Uninitializing COM...");
        ole32::CoUninitialize();
        result.unwrap();
        debug!(logger, "Completed successfully");
    }
}
