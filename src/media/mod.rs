//! Provides a Rust layer over the Windows IMMDevice API.

#![allow(unknown_lints)]

mod event;

use std::alloc;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::isize;
use std::mem::MaybeUninit;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr;
use std::slice;
use std::sync::atomic::{self, AtomicUsize, Ordering};

use regex::Regex;
use tracing::{info, instrument};
use winapi::ctypes::c_void;
use winapi::shared::guiddef::GUID;
use winapi::shared::guiddef::{IsEqualIID, REFIID};
use winapi::shared::minwindef::ULONG;
use winapi::shared::ntdef::HRESULT;
use winapi::shared::winerror::{E_INVALIDARG, E_NOINTERFACE};
use winapi::shared::wtypes::{PROPERTYKEY, VARTYPE};
use winapi::um::combaseapi::CLSCTX_ALL;
use winapi::um::combaseapi::{CoCreateInstance, CoTaskMemFree, PropVariantClear};
use winapi::um::coml2api::STGM_READ;
use winapi::um::endpointvolume::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallbackVtbl,
    AUDIO_VOLUME_NOTIFICATION_DATA,
};
use winapi::um::mmdeviceapi::{
    eConsole, eRender, CLSID_MMDeviceEnumerator, IMMDevice, IMMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use winapi::um::propidl::PROPVARIANT;
use winapi::um::propsys::IPropertyStore;
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::Interface;

pub(crate) use self::event::VolumeEvents;
pub use self::event::VolumeNotification;
use crate::com::{ComObject, ComScope};
use crate::hresult::{check, Win32Error};
use crate::lazy::Lazy;
use crate::soundcore::{SoundCoreError, PKEY_SOUNDCORECTL_CLSID_AE5, PKEY_SOUNDCORECTL_CLSID_Z};
use crate::winapiext::{PKEY_DeviceInterface_FriendlyName, PKEY_Device_DeviceDesc};

fn parse_guid(src: &str) -> Result<GUID, Box<dyn Error>> {
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
    let mut array = [0; 8];
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
    volume: Lazy<Result<ComObject<IAudioEndpointVolume>, Win32Error>>,
    properties: Lazy<Result<PropertyStore, Win32Error>>,
}

impl Endpoint {
    fn new(device: ComObject<IMMDevice>) -> Self {
        Self {
            device,
            volume: Lazy::new(),
            properties: Lazy::new(),
        }
    }
    /// Gets the ID of the endpoint.
    ///
    /// See [Endpoint ID Strings](https://docs.microsoft.com/en-us/windows/desktop/CoreAudio/endpoint-id-strings).
    #[instrument(level = "trace")]
    pub fn id(&self) -> Result<String, Win32Error> {
        unsafe {
            let mut raw_id = MaybeUninit::uninit();
            check(self.device.GetId(raw_id.as_mut_ptr()))?;
            let raw_id = raw_id.assume_init();
            let length = (0..isize::MAX)
                .position(|i| *raw_id.offset(i) == 0)
                .unwrap();
            let str: OsString = OsStringExt::from_wide(slice::from_raw_parts(raw_id, length));
            CoTaskMemFree(raw_id as *mut _);
            Ok(str.to_string_lossy().into_owned())
        }
    }
    #[instrument(level = "trace")]
    fn property_store(&self) -> Result<&PropertyStore, Win32Error> {
        self.properties
            .get_or_create(|| unsafe {
                let mut property_store = MaybeUninit::uninit();
                check(
                    self.device
                        .OpenPropertyStore(STGM_READ, property_store.as_mut_ptr()),
                )?;
                Ok(PropertyStore(ComObject::take(property_store.assume_init())))
            })
            .as_ref()
            .map_err(|e| e.clone())
    }
    /// Gets the CLSID of the class implementing Creative's APIs.
    ///
    /// This allows discovery of a SoundCore implementation for devices that support it.
    pub fn clsid(&self) -> Result<GUID, SoundCoreError> {
        let store = self.property_store()?;
        let value = match store.get_string_value(&PKEY_SOUNDCORECTL_CLSID_AE5)? {
            Some(value) => value,
            None => store
                .get_string_value(&PKEY_SOUNDCORECTL_CLSID_Z)?
                .ok_or(SoundCoreError::NotSupported)?,
        };
        parse_guid(&value).or(Err(SoundCoreError::NotSupported))
    }

    /// Gets the friendly name of the audio interface (sound adapter).
    ///
    /// See [Core Audio Properties: Device Properties](https://docs.microsoft.com/en-us/windows/desktop/coreaudio/core-audio-properties#device-properties).
    pub fn interface(&self) -> Result<String, GetPropertyError> {
        self.property_store()?
            .get_string_value(&PKEY_DeviceInterface_FriendlyName)?
            .ok_or(GetPropertyError::NOT_FOUND)
    }
    /// Gets a description of the audio endpoint (speakers, headphones, etc).
    ///
    /// See [Core Audio Properties: Device Properties](https://docs.microsoft.com/en-us/windows/desktop/coreaudio/core-audio-properties#device-properties).
    pub fn description(&self) -> Result<String, GetPropertyError> {
        self.property_store()?
            .get_string_value(&PKEY_Device_DeviceDesc)?
            .ok_or(GetPropertyError::NOT_FOUND)
    }
    fn volume(&self) -> Result<ComObject<IAudioEndpointVolume>, Win32Error> {
        self.volume
            .get_or_create(|| unsafe {
                let mut ctrl = MaybeUninit::<*mut IAudioEndpointVolume>::uninit();
                check(self.device.Activate(
                    &IAudioEndpointVolume::uuidof(),
                    CLSCTX_ALL,
                    ptr::null_mut(),
                    ctrl.as_mut_ptr() as *mut _,
                ))?;
                Ok(ComObject::take(ctrl.assume_init()))
            })
            .clone()
    }
    /// Checks whether the device is already muted.
    #[instrument(level = "debug")]
    pub fn get_mute(&self) -> Result<bool, Win32Error> {
        unsafe {
            let mut mute = 0;
            check(self.volume()?.GetMute(&mut mute))?;
            Ok(mute != 0)
        }
    }
    /// Mutes or unmutes the device.
    #[instrument]
    pub fn set_mute(&self, mute: bool) -> Result<(), Win32Error> {
        unsafe {
            let mute = if mute { 1 } else { 0 };
            check(self.volume()?.SetMute(mute, ptr::null_mut()))?;
            Ok(())
        }
    }
    /// Sets the volume of the device.
    ///
    /// Volumes range from 0.0 to 1.0.
    ///
    /// Volume can be controlled independent of muting.
    #[instrument]
    pub fn set_volume(&self, volume: f32) -> Result<(), Win32Error> {
        unsafe {
            info!("Setting volume to {volume}...");
            check(
                self.volume()?
                    .SetMasterVolumeLevelScalar(volume, ptr::null_mut()),
            )?;
            Ok(())
        }
    }
    /// Gets the volume of the device.
    #[instrument(level = "debug", fields(volume))]
    pub fn get_volume(&self) -> Result<f32, Win32Error> {
        unsafe {
            let mut volume = MaybeUninit::uninit();
            check(
                self.volume()?
                    .GetMasterVolumeLevelScalar(volume.as_mut_ptr()),
            )?;
            let volume = volume.assume_init();
            tracing::Span::current().record("volume", &volume);
            Ok(volume)
        }
    }
    pub(crate) fn event_stream(&self) -> Result<VolumeEvents, Win32Error> {
        VolumeEvents::new(self.volume()?)
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("device", &self.device)
            .finish()
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

impl GetPropertyError {
    pub(crate) const NOT_FOUND: GetPropertyError = GetPropertyError::UnexpectedType(0);
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
    fn cause(&self) -> Option<&dyn Error> {
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

#[derive(Debug)]
struct PropertyStore(ComObject<IPropertyStore>);

impl PropertyStore {
    #[instrument(level = "trace", skip(key), fields(key.fmtid, key.pid))]
    unsafe fn get_value(&self, key: &PROPERTYKEY) -> Result<PROPVARIANT, Win32Error> {
        let mut property_value = MaybeUninit::uninit();
        check(self.0.GetValue(key, property_value.as_mut_ptr()))?;
        Ok(property_value.assume_init())
    }
    #[allow(clippy::cast_ptr_alignment)]
    #[instrument(level = "trace", skip(key), fields(key.fmtid, key.pid, r#type, value))]
    fn get_string_value(&self, key: &PROPERTYKEY) -> Result<Option<String>, GetPropertyError> {
        unsafe {
            let mut property_value = self.get_value(key)?;
            trace!(self.1, "Returned variant has type {}", property_value.vt);
            // VT_EMPTY
            if property_value.vt == 0 {
                return Ok(None);
            }
            // VT_LPWSTR
            if property_value.vt != 31 {
                PropVariantClear(&mut property_value);
                return Err(GetPropertyError::UnexpectedType(property_value.vt));
            }
            let chars = *property_value.data.pwszVal();
            let length = (0..isize::MAX).position(|i| *chars.offset(i) == 0);
            let str = length.map(|length| {
                OsString::from_wide(slice::from_raw_parts(chars, length))
                    .to_string_lossy()
                    .into_owned()
            });
            PropVariantClear(&mut property_value);
            let str = str.unwrap();
            tracing::Span::current().record("value", &str.as_str());
            Ok(Some(str))
        }
    }
}

/// Provides access to the devices available in the current Windows session.
#[derive(Debug)]
pub struct DeviceEnumerator(ComObject<IMMDeviceEnumerator>);

impl DeviceEnumerator {
    /// Creates a new device enumerator.
    #[instrument(level = "trace")]
    pub fn new() -> Result<Self, Win32Error> {
        unsafe {
            let _scope = ComScope::begin();
            let mut enumerator = MaybeUninit::<*mut IMMDeviceEnumerator>::uninit();
            check(CoCreateInstance(
                &CLSID_MMDeviceEnumerator,
                ptr::null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::uuidof(),
                enumerator.as_mut_ptr() as *mut _,
            ))?;
            Ok(DeviceEnumerator(ComObject::take(enumerator.assume_init())))
        }
    }
    /// Gets all active audio outputs.
    #[allow(clippy::unnecessary_mut_passed)]
    #[instrument(level = "trace")]
    pub fn get_active_audio_endpoints(&self) -> Result<Vec<Endpoint>, Win32Error> {
        unsafe {
            let mut collection = MaybeUninit::uninit();
            check(self.0.EnumAudioEndpoints(
                eRender,
                DEVICE_STATE_ACTIVE,
                collection.as_mut_ptr(),
            ))?;
            let collection = collection.assume_init();
            let mut count = 0;
            check((*collection).GetCount(&mut count))?;
            let mut result = Vec::with_capacity(count as usize);
            for i in 0..count {
                let mut device = MaybeUninit::uninit();
                check((*collection).Item(i, device.as_mut_ptr()))?;
                result.push(Endpoint::new(ComObject::take(device.assume_init())))
            }
            Ok(result)
        }
    }
    /// Gets the default audio output.
    ///
    /// There are multiple default audio outputs in Windows.
    /// This function gets the device that would be used if the current application
    /// were to play music or sound effects (as opposed to VOIP audio).
    #[instrument(level = "trace")]
    pub fn get_default_audio_endpoint(&self) -> Result<Endpoint, Win32Error> {
        unsafe {
            let mut device = MaybeUninit::uninit();
            check(
                self.0
                    .GetDefaultAudioEndpoint(eRender, eConsole, device.as_mut_ptr()),
            )?;
            Ok(Endpoint::new(ComObject::take(device.assume_init())))
        }
    }
    /// Get a specific audio endpoint by its ID.
    #[instrument(level = "trace")]
    pub fn get_endpoint(&self, id: &OsStr) -> Result<Endpoint, Win32Error> {
        let buffer: Vec<_> = id.encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut device = MaybeUninit::uninit();
            check(self.0.GetDevice(buffer.as_ptr(), device.as_mut_ptr()))?;
            Ok(Endpoint::new(ComObject::take(device.assume_init())))
        }
    }
}

#[repr(C)]
struct AudioEndpointVolumeCallback<C> {
    lp_vtbl: *mut IAudioEndpointVolumeCallbackVtbl,
    vtbl: IAudioEndpointVolumeCallbackVtbl,
    refs: AtomicUsize,
    callback: C,
}

impl<C> AudioEndpointVolumeCallback<C>
where
    C: Send + 'static + FnMut(&AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<(), Win32Error>,
{
    /// Wraps a function in an `IAudioEndpointVolumeCallback`.
    pub unsafe fn wrap(callback: C) -> *mut IAudioEndpointVolumeCallback {
        let mut value = Box::new(AudioEndpointVolumeCallback::<C> {
            lp_vtbl: ptr::null_mut(),
            vtbl: IAudioEndpointVolumeCallbackVtbl {
                parent: IUnknownVtbl {
                    QueryInterface: callback_query_interface::<C>,
                    AddRef: callback_add_ref::<C>,
                    Release: callback_release::<C>,
                },
                OnNotify: callback_on_notify::<C>,
            },
            refs: AtomicUsize::new(1),
            callback,
        });
        value.lp_vtbl = &mut value.vtbl as *mut _;
        Box::into_raw(value) as *mut _
    }
}

// ensures `this` is an instance of the expected type
unsafe fn validate<I, C>(this: *mut I) -> Result<*mut AudioEndpointVolumeCallback<C>, Win32Error>
where
    I: Interface,
{
    let this = this as *mut IUnknown;
    if this.is_null()
        || (*this).lpVtbl.is_null()
        || (*(*this).lpVtbl).QueryInterface as usize != callback_query_interface::<C> as usize
    {
        Err(Win32Error::new(E_INVALIDARG))
    } else {
        Ok(this as *mut AudioEndpointVolumeCallback<C>)
    }
}

// converts a `Result` to an `HRESULT` so `?` can be used
unsafe fn uncheck<E>(result: E) -> HRESULT
where
    E: FnOnce() -> Result<HRESULT, Win32Error>,
{
    match result() {
        Ok(result) => result,
        Err(Win32Error { code, .. }) => code,
    }
}

unsafe extern "system" fn callback_query_interface<C>(
    this: *mut IUnknown,
    iid: REFIID,
    object: *mut *mut c_void,
) -> HRESULT {
    uncheck(|| {
        let this = validate::<_, C>(this)?;
        let iid = iid.as_ref().unwrap();
        if IsEqualIID(iid, &IUnknown::uuidof())
            || IsEqualIID(iid, &IAudioEndpointVolumeCallback::uuidof())
        {
            (*this).refs.fetch_add(1, Ordering::Relaxed);
            *object = this as *mut c_void;
            Ok(0)
        } else {
            *object = ptr::null_mut();
            Err(Win32Error::new(E_NOINTERFACE))
        }
    })
}

unsafe extern "system" fn callback_add_ref<C>(this: *mut IUnknown) -> ULONG {
    match validate::<_, C>(this) {
        Ok(this) => {
            let count = (*this).refs.fetch_add(1, Ordering::Relaxed) + 1;
            count as ULONG
        }
        Err(_) => 1,
    }
}

unsafe extern "system" fn callback_release<C>(this: *mut IUnknown) -> ULONG {
    match validate::<_, C>(this) {
        Ok(this) => {
            let count = (*this).refs.fetch_sub(1, Ordering::Release) - 1;
            if count == 0 {
                atomic::fence(Ordering::Acquire);
                ptr::drop_in_place(this);
                alloc::dealloc(
                    this as *mut u8,
                    alloc::Layout::for_value(this.as_ref().unwrap()),
                );
            }
            count as ULONG
        }
        Err(_) => 1,
    }
}

unsafe extern "system" fn callback_on_notify<C>(
    this: *mut IAudioEndpointVolumeCallback,
    notify: *mut AUDIO_VOLUME_NOTIFICATION_DATA,
) -> HRESULT
where
    C: FnMut(&AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<(), Win32Error>,
{
    uncheck(|| {
        let this = validate::<_, C>(this)?;
        ((*this).callback)(&*notify)?;
        Ok(0)
    })
}
