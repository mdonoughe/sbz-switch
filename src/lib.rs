#![warn(missing_docs)]

//! Provides a high-level API for controlling Creative sound devices.
//!
//! For a lower-level API, see [`media`](media/index.html) and [`soundcore`](soundcore/index.html).
//!
//! For an even-lower-level API, see [`mmdeviceapi`](../winapi/um/mmdeviceapi/index.html) and [`ctsndcr`](ctsndcr/index.html).

extern crate indexmap;
extern crate regex;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate winapi;

mod com;
pub mod ctsndcr;
mod hresult;
mod lazy;
pub mod media;
pub mod soundcore;
mod winapiext;

use futures::stream::Fuse;
use futures::task::Context;
use futures::{Stream, StreamExt};

use indexmap::IndexMap;

use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::pin::Pin;
use std::task::Poll;

use slog::Logger;

use crate::com::event::ComEventIterator;
use crate::media::{DeviceEnumerator, Endpoint, VolumeEvents, VolumeNotification};
use crate::soundcore::{
    SoundCore, SoundCoreEvent, SoundCoreEventIterator, SoundCoreEvents, SoundCoreFeature,
    SoundCoreParamValue, SoundCoreParameter,
};

pub use crate::hresult::Win32Error;

#[cfg(not(any(target_arch = "x86", feature = "ctsndcr_ignore_arch")))]
compile_error!("This crate must be built for x86 for compatibility with sound drivers." +
    "(build for i686-pc-windows-msvc or suppress this error using feature ctsndcr_ignore_arch)");

/// Describes the configuration of a media endpoint.
#[derive(Debug)]
pub struct EndpointConfiguration {
    /// The desired volume level, from 0.0 to 1.0
    pub volume: Option<f32>,
}

/// Describes a configuration to be applied.
#[derive(Debug)]
pub struct Configuration {
    /// Windows audio endpoint settings
    pub endpoint: Option<EndpointConfiguration>,
    /// Creative SoundBlaster settings
    pub creative: Option<IndexMap<String, IndexMap<String, SoundCoreParamValue>>>,
}

/// Describes a device that may be configurable.
pub struct DeviceInfo {
    /// Represents the device to Windows.
    pub id: String,
    /// Describes the hardware that connects the device to the computer.
    pub interface: String,
    /// Describes the audio device.
    pub description: String,
}

/// Produces a list of devices currently available.
///
/// This may include devices that are not configurable.
///
/// # Examples
///
/// ```
/// for device in list_devices(logger.clone())? {
///     println!("{}: {}", device.id, device.description);
/// }
/// ```
pub fn list_devices(logger: &Logger) -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
    let endpoints = DeviceEnumerator::with_logger(logger.clone())?.get_active_audio_endpoints()?;
    let mut result = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let id = endpoint.id()?;
        debug!(logger, "Querying endpoint {}...", id);
        result.push(DeviceInfo {
            id,
            interface: endpoint.interface()?,
            description: endpoint.description()?,
        })
    }
    Ok(result)
}

fn get_endpoint(logger: Logger, device_id: Option<&OsStr>) -> Result<Endpoint, Win32Error> {
    let enumerator = DeviceEnumerator::with_logger(logger)?;
    Ok(match device_id {
        Some(id) => enumerator.get_endpoint(id)?,
        None => enumerator.get_default_audio_endpoint()?,
    })
}

/// Captures a snapshot of a device's configuration.
///
/// If `device_id` is `None`, the system default output device will be used.
///
/// # Examples
///
/// ```
/// println!("{:?}", dump(logger.clone(), None)?);
/// ```
pub fn dump(logger: &Logger, device_id: Option<&OsStr>) -> Result<Configuration, Box<dyn Error>> {
    let endpoint = get_endpoint(logger.clone(), device_id)?;

    let endpoint_output = EndpointConfiguration {
        volume: Some(endpoint.get_volume()?),
    };

    let id = endpoint.id()?;
    debug!(logger, "Found device {}", id);
    let clsid = endpoint.clsid()?;
    debug!(
        logger,
        "Found clsid {{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        clsid.Data1,
        clsid.Data2,
        clsid.Data3,
        clsid.Data4[0],
        clsid.Data4[1],
        clsid.Data4[2],
        clsid.Data4[3],
        clsid.Data4[4],
        clsid.Data4[5],
        clsid.Data4[6],
        clsid.Data4[7]
    );
    let core = SoundCore::for_device(&clsid, &id, logger.clone())?;

    let mut context_output = IndexMap::new();
    for feature in core.features(0) {
        let feature = feature?;
        debug!(logger, "{:08x} {}", feature.id, feature.description);

        let mut feature_output = IndexMap::new();
        for parameter in feature.parameters() {
            let parameter = parameter?;
            debug!(logger, "  {} {}", parameter.id, parameter.description);
            debug!(logger, "    attributes: {}", parameter.attributes);
            if let Some(size) = parameter.size {
                debug!(logger, "    size:       {}", size);
            }
            // skip read-only parameters
            if parameter.attributes & 1 == 0 {
                match parameter.kind {
                    1 => {
                        let value = parameter.get()?;
                        debug!(logger, "    value:      {:?}", value);
                        match value {
                            SoundCoreParamValue::None => {}
                            _ => {
                                feature_output.insert(parameter.description.clone(), value);
                            }
                        }
                    }
                    0 | 2 | 3 => {
                        let value = parameter.get()?;
                        debug!(logger, "    minimum:    {:?}", parameter.min_value);
                        debug!(logger, "    maximum:    {:?}", parameter.max_value);
                        debug!(logger, "    step:       {:?}", parameter.step_size);
                        debug!(logger, "    value:      {:?}", value);
                        match value {
                            SoundCoreParamValue::None => {}
                            _ => {
                                feature_output.insert(parameter.description.clone(), value);
                            }
                        }
                    }
                    5 => {}
                    _ => {
                        debug!(logger, "     kind:      {}", parameter.kind);
                    }
                }
            }
        }
        // omit feature if no parameters are applicable
        if !feature_output.is_empty() {
            context_output.insert(feature.description.clone(), feature_output);
        }
    }

    Ok(Configuration {
        endpoint: Some(endpoint_output),
        creative: Some(context_output),
    })
}

/// Applies a set of configuration values to a device.
///
/// If `device_id` is None, the system default output device will be used.
///
/// `mute` controls whether the device is muted at the start of the operation
/// and unmuted at the end. In any case, the device will not be unmuted if it
/// was already muted before calling this function.
///
/// # Examples
///
/// ```
/// let mut creative = BTreeMap::<String, BTreeMap<String, Value>>::new();
/// let mut device_control = BTreeMap::<String, Value>::new();
/// device_control.insert("SelectOutput".to_string(), Value::Integer(1));
/// let configuration = Configuration {
///     endpoint: None,
///     creative,
/// };
/// set(logger.clone(), None, &configuration, true);
/// ```
pub fn set(
    logger: &Logger,
    device_id: Option<&OsStr>,
    configuration: &Configuration,
    mute: bool,
) -> Result<(), Box<dyn Error>> {
    let endpoint = get_endpoint(logger.clone(), device_id)?;
    let mute_unmute = mute && !endpoint.get_mute()?;
    if mute_unmute {
        endpoint.set_mute(true)?;
    }
    let result = set_internal(logger, configuration, &endpoint);
    if mute_unmute {
        endpoint.set_mute(false)?;
    }

    result
}

/// Gets the sequence of events for a device.
///
/// If `device_id` is None, the system default output device will be used.
///
/// # Examples
///
/// ```
/// for event in watch(logger.clone(), None) {
///     println!("{:?}", event);
/// }
/// ```
pub fn watch(
    logger: &Logger,
    device_id: Option<&OsStr>,
) -> Result<SoundCoreEventIterator, Box<dyn Error>> {
    let endpoint = get_endpoint(logger.clone(), device_id)?;
    let id = endpoint.id()?;
    let clsid = endpoint.clsid()?;
    let core = SoundCore::for_device(&clsid, &id, logger.clone())?;

    Ok(core.events()?)
}

/// Either a SoundCoreEvent or a VolumeNotification.
#[derive(Debug)]
pub enum SoundCoreOrVolumeEvent {
    /// A SoundCoreEvent.
    SoundCore(SoundCoreEvent),
    /// A VolumeNotification.
    Volume(VolumeNotification),
}

struct SoundCoreAndVolumeEvents {
    sound_core: Fuse<SoundCoreEvents>,
    volume: Fuse<VolumeEvents>,
}

impl Stream for SoundCoreAndVolumeEvents {
    type Item = Result<SoundCoreOrVolumeEvent, Win32Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(Some(item)) = Pin::new(&mut self.sound_core).poll_next(cx) {
            Poll::Ready(Some(match item {
                Ok(item) => Ok(SoundCoreOrVolumeEvent::SoundCore(item)),
                Err(err) => Err(err),
            }))
        } else if let Poll::Ready(Some(item)) = Pin::new(&mut self.volume).poll_next(cx) {
            Poll::Ready(Some(Ok(SoundCoreOrVolumeEvent::Volume(item))))
        } else {
            Poll::Pending
        }
    }
}

/// Iterates over volume change events and also events produced through the
/// SoundCore API.
///
/// This iterator will block until the next event is available.
pub struct SoundCoreAndVolumeEventIterator {
    inner: ComEventIterator<SoundCoreAndVolumeEvents>,
}

impl Iterator for SoundCoreAndVolumeEventIterator {
    type Item = Result<SoundCoreOrVolumeEvent, Win32Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// Gets the sequence of events for a device.
///
/// If `device_id` is None, the system default output device will be used.
///
/// # Examples
///
/// ```
/// for event in watch_with_volume(logger.clone(), None) {
///     println!("{:?}", event);
/// }
/// ```
pub fn watch_with_volume(
    logger: &Logger,
    device_id: Option<&OsStr>,
) -> Result<SoundCoreAndVolumeEventIterator, Box<dyn Error>> {
    let endpoint = get_endpoint(logger.clone(), device_id)?;
    let id = endpoint.id()?;
    let clsid = endpoint.clsid()?;
    let core = SoundCore::for_device(&clsid, &id, logger.clone())?;

    let core_events = core.event_stream()?;
    let volume_events = endpoint.event_stream()?;

    Ok(SoundCoreAndVolumeEventIterator {
        inner: ComEventIterator::new(SoundCoreAndVolumeEvents {
            sound_core: core_events.fuse(),
            volume: volume_events.fuse(),
        }),
    })
}

#[derive(Debug)]
struct UnsupportedValueError {
    feature: String,
    parameter: String,
    expected: &'static str,
    actual: &'static str,
}

impl fmt::Display for UnsupportedValueError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Unsupported value for {}.{}. Expected {}, got {}.",
            self.feature, self.parameter, self.expected, self.actual
        )
    }
}

impl Error for UnsupportedValueError {
    fn description(&self) -> &str {
        "The provided value was not compatible with the specified parameter."
    }

    fn cause(&self) -> Option<&dyn Error> {
        None
    }
}

fn coerce_soundcore(
    feature: &SoundCoreFeature,
    parameter: &SoundCoreParameter,
    value: &SoundCoreParamValue,
) -> Result<SoundCoreParamValue, UnsupportedValueError> {
    match (value, parameter.kind) {
        (&SoundCoreParamValue::Float(f), 0) => Ok(SoundCoreParamValue::Float(f)),
        (&SoundCoreParamValue::U32(i), 0) => Ok(SoundCoreParamValue::Float(i as f32)),
        (&SoundCoreParamValue::I32(i), 0) => Ok(SoundCoreParamValue::Float(i as f32)),
        (&SoundCoreParamValue::Bool(b), 1) => Ok(SoundCoreParamValue::Bool(b)),
        (&SoundCoreParamValue::U32(i), 2) => Ok(SoundCoreParamValue::U32(i)),
        (&SoundCoreParamValue::I32(i), 2) if 0 <= i => Ok(SoundCoreParamValue::U32(i as u32)),
        (&SoundCoreParamValue::I32(i), 3) => Ok(SoundCoreParamValue::I32(i)),
        (&SoundCoreParamValue::U32(i), 3) if i <= i32::max_value() as u32 => {
            Ok(SoundCoreParamValue::I32(i as i32))
        }
        _ => {
            let actual = match *value {
                SoundCoreParamValue::Float(_) => "float",
                SoundCoreParamValue::Bool(_) => "bool",
                SoundCoreParamValue::I32(_) => "int",
                SoundCoreParamValue::U32(_) => "uint",
                SoundCoreParamValue::None => "<unsupported>",
            };
            Err(UnsupportedValueError {
                feature: feature.description.to_owned(),
                parameter: parameter.description.to_owned(),
                expected: match parameter.kind {
                    0 => "float",
                    1 => "bool",
                    2 => "uint",
                    3 => "int",
                    _ => "<unsupported>",
                },
                actual,
            })
        }
    }
}

fn set_internal(
    logger: &Logger,
    configuration: &Configuration,
    endpoint: &Endpoint,
) -> Result<(), Box<dyn Error>> {
    if let Some(ref creative) = configuration.creative {
        let id = endpoint.id()?;
        debug!(logger, "Found device {}", id);
        let clsid = endpoint.clsid()?;
        debug!(
            logger,
            "Found clsid \
             {{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            clsid.Data1,
            clsid.Data2,
            clsid.Data3,
            clsid.Data4[0],
            clsid.Data4[1],
            clsid.Data4[2],
            clsid.Data4[3],
            clsid.Data4[4],
            clsid.Data4[5],
            clsid.Data4[6],
            clsid.Data4[7]
        );
        let core = SoundCore::for_device(&clsid, &id, logger.clone())?;

        let mut unhandled_feature_names = BTreeSet::<&str>::new();
        for (key, _) in creative.iter() {
            unhandled_feature_names.insert(key);
        }

        for feature in core.features(0) {
            let feature = feature?;
            trace!(logger, "Looking for {} settings...", feature.description);
            if let Some(feature_table) = creative.get(&feature.description) {
                unhandled_feature_names.remove(&feature.description[..]);
                let mut unhandled_parameter_names = BTreeSet::<&str>::new();
                for (key, _) in feature_table.iter() {
                    unhandled_parameter_names.insert(key);
                }

                for parameter in feature.parameters() {
                    let mut parameter = parameter?;
                    trace!(
                        logger,
                        "Looking for {}.{} settings...",
                        feature.description,
                        parameter.description
                    );
                    if let Some(value) = feature_table.get(&parameter.description) {
                        unhandled_parameter_names.remove(&parameter.description[..]);
                        let value = &coerce_soundcore(&feature, &parameter, value)?;
                        if let Err(error) = parameter.set(value) {
                            error!(
                                logger,
                                "Could not set parameter {}.{}: {}",
                                feature.description,
                                parameter.description,
                                error
                            );
                        }
                    }
                }
                for unhandled in unhandled_parameter_names {
                    warn!(
                        logger,
                        "Could not find parameter {}.{}", feature.description, unhandled
                    );
                }
            }
        }
        for unhandled in unhandled_feature_names {
            warn!(logger, "Could not find feature {}", unhandled);
        }
    }
    if let Some(ref endpoint_config) = configuration.endpoint {
        if let Some(v) = endpoint_config.volume {
            endpoint.set_volume(v)?;
        }
    }
    Ok(())
}
