//! Provides a Rust layer over the Windows IMMDevice API.

#![allow(unknown_lints)]

mod event;

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Debug};
use std::isize;
use std::os::windows::ffi::OsStringExt;
use std::slice;
use std::sync::Mutex;

use futures::channel::mpsc::UnboundedSender;
use futures::{executor, SinkExt};
use regex::Regex;
use tracing::{info, instrument};
use windows::core::{implement, GUID, PCWSTR};
use windows::Win32::Foundation::E_ABORT;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ, VT_EMPTY, VT_LPWSTR,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, PROPERTYKEY};

pub(crate) use self::event::VolumeEvents;
pub use self::event::VolumeNotification;
use crate::com::{ComObject, ComScope};
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
        data1: l,
        data2: w1,
        data3: w2,
        data4: array,
    })
}

/// Represents an audio device.
pub struct Endpoint {
    device: ComObject<IMMDevice>,
    volume: Lazy<windows::core::Result<ComObject<IAudioEndpointVolume>>>,
    properties: Lazy<windows::core::Result<PropertyStore>>,
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
    pub fn id(&self) -> windows::core::Result<String> {
        unsafe {
            let raw_id = self.device.GetId()?.0;
            let length = (0..isize::MAX)
                .position(|i| *raw_id.offset(i) == 0)
                .unwrap();
            let str: OsString = OsStringExt::from_wide(slice::from_raw_parts(raw_id, length));
            CoTaskMemFree(raw_id as *mut _);
            Ok(str.to_string_lossy().into_owned())
        }
    }
    #[instrument(level = "trace")]
    fn property_store(&self) -> windows::core::Result<&PropertyStore> {
        self.properties
            .get_or_create(|| unsafe {
                let property_store = self.device.OpenPropertyStore(STGM_READ)?;
                Ok(PropertyStore(ComObject::take(property_store)))
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
    fn volume(&self) -> windows::core::Result<ComObject<IAudioEndpointVolume>> {
        self.volume
            .get_or_create(|| unsafe {
                let ctrl = self.device.Activate(CLSCTX_ALL, None)?;
                Ok(ComObject::take(ctrl))
            })
            .clone()
    }
    /// Checks whether the device is already muted.
    #[instrument(level = "debug")]
    pub fn get_mute(&self) -> windows::core::Result<bool> {
        unsafe { Ok(self.volume()?.GetMute()?.into()) }
    }
    /// Mutes or unmutes the device.
    #[instrument]
    pub fn set_mute(&self, mute: bool) -> windows::core::Result<()> {
        unsafe {
            self.volume()?.SetMute(mute, &GUID::zeroed())?;
            Ok(())
        }
    }
    /// Sets the volume of the device.
    ///
    /// Volumes range from 0.0 to 1.0.
    ///
    /// Volume can be controlled independent of muting.
    #[instrument]
    pub fn set_volume(&self, volume: f32) -> windows::core::Result<()> {
        unsafe {
            info!("Setting volume to {volume}...");
            self.volume()?
                .SetMasterVolumeLevelScalar(volume, &GUID::zeroed())?;
            Ok(())
        }
    }
    /// Gets the volume of the device.
    #[instrument(level = "debug", fields(volume))]
    pub fn get_volume(&self) -> windows::core::Result<f32> {
        unsafe {
            let volume = self.volume()?.GetMasterVolumeLevelScalar()?;
            tracing::Span::current().record("volume", &volume);
            Ok(volume)
        }
    }
    pub(crate) fn event_stream(&self) -> windows::core::Result<VolumeEvents> {
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
    Win32(windows::core::Error),
    /// The returned value was not the expected type.
    UnexpectedType(u16),
}

impl GetPropertyError {
    pub(crate) const NOT_FOUND: GetPropertyError = GetPropertyError::UnexpectedType(0);
}

impl fmt::Display for GetPropertyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GetPropertyError::Win32(error) => write!(f, "{}", error),
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

impl From<windows::core::Error> for GetPropertyError {
    fn from(error: windows::core::Error) -> Self {
        GetPropertyError::Win32(error)
    }
}

#[derive(Debug)]
struct PropertyStore(ComObject<IPropertyStore>);

impl PropertyStore {
    unsafe fn get_value(&self, key: &PROPERTYKEY) -> windows::core::Result<PROPVARIANT> {
        self.0.GetValue(key)
    }
    #[allow(clippy::cast_ptr_alignment)]
    #[instrument(level = "trace", skip(key), fields(key.fmtid = ?key.fmtid, key.pid = %key.pid, r#type, value))]
    fn get_string_value(&self, key: &PROPERTYKEY) -> Result<Option<String>, GetPropertyError> {
        unsafe {
            let mut property_value = self.get_value(key)?;
            tracing::Span::current().record("type", &property_value.Anonymous.Anonymous.vt.0);
            if property_value.Anonymous.Anonymous.vt == VT_EMPTY {
                return Ok(None);
            }
            if property_value.Anonymous.Anonymous.vt != VT_LPWSTR {
                PropVariantClear(&mut property_value).unwrap();
                return Err(GetPropertyError::UnexpectedType(
                    property_value.Anonymous.Anonymous.vt.0,
                ));
            }
            let chars = property_value.Anonymous.Anonymous.Anonymous.pwszVal.0;
            let length = (0..isize::MAX).position(|i| *chars.offset(i) == 0);
            let str = length.map(|length| {
                OsString::from_wide(slice::from_raw_parts(chars, length))
                    .to_string_lossy()
                    .into_owned()
            });
            PropVariantClear(&mut property_value).unwrap();
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
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let _scope = ComScope::begin();
            let enumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            Ok(DeviceEnumerator(ComObject::take(enumerator)))
        }
    }
    /// Gets all active audio outputs.
    #[allow(clippy::unnecessary_mut_passed)]
    #[instrument(level = "trace")]
    pub fn get_active_audio_endpoints(&self) -> windows::core::Result<Vec<Endpoint>> {
        unsafe {
            let collection = self.0.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            let count = collection.GetCount()?;
            let mut result = Vec::with_capacity(count as usize);
            for i in 0..count {
                let device = collection.Item(i)?;
                result.push(Endpoint::new(ComObject::take(device)))
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
    pub fn get_default_audio_endpoint(&self) -> windows::core::Result<Endpoint> {
        unsafe {
            let device = self.0.GetDefaultAudioEndpoint(eRender, eConsole)?;
            Ok(Endpoint::new(ComObject::take(device)))
        }
    }
    /// Get a specific audio endpoint by its ID.
    #[instrument(level = "trace", skip(id), fields(id))]
    pub fn get_endpoint<I>(&self, id: I) -> windows::core::Result<Endpoint>
    where
        I: Into<PCWSTR>,
    {
        unsafe {
            let id: PCWSTR = id.into();
            tracing::Span::current().record("id", &tracing::field::display(id.display()));
            let device = self.0.GetDevice(id)?;
            Ok(Endpoint::new(ComObject::take(device)))
        }
    }
}

#[implement(IAudioEndpointVolumeCallback)]
pub(crate) struct AudioEndpointVolumeCallback {
    sender: Mutex<UnboundedSender<VolumeNotification>>,
}

impl AudioEndpointVolumeCallback {
    unsafe fn new(sender: UnboundedSender<VolumeNotification>) -> Self {
        Self {
            sender: Mutex::new(sender),
        }
    }
}

impl IAudioEndpointVolumeCallback_Impl for AudioEndpointVolumeCallback {
    fn OnNotify(&self, notify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        unsafe {
            match executor::block_on(self.sender.lock().unwrap().send(VolumeNotification {
                event_context: (*notify).guidEventContext,
                is_muted: (*notify).bMuted.into(),
                volume: (*notify).fMasterVolume,
            })) {
                Ok(()) => Ok(()),
                Err(_) => Err(windows::core::Error::from(E_ABORT)),
            }
        }
    }
}
