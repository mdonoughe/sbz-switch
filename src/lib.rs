#![warn(missing_docs)]

//! Provides a high-level API for controlling Creative sound devices.
//!
//! For a lower-level API, see [`media`](media/index.html) and [`soundcore`](soundcore/index.html).
//!
//! For an even-lower-level API, see [`mmdeviceapi`](../winapi/um/mmdeviceapi/index.html) and [`ctsndcr`](ctsndcr/index.html).

mod com;
pub mod ctsndcr;
mod lazy;
pub mod media;
pub mod soundcore;
mod winapiext;

use futures::stream::Fuse;
use futures::task::Context;
use futures::{Stream, StreamExt};

use indexmap::IndexMap;
use tracing::{debug, debug_span, error, trace_span, warn};

use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::pin::Pin;
use std::task::Poll;

use crate::com::event::ComEventIterator;
use crate::media::{DeviceEnumerator, Endpoint, VolumeEvents, VolumeNotification};
use crate::soundcore::{
    SoundCore, SoundCoreEvent, SoundCoreEventIterator, SoundCoreEvents, SoundCoreFeature,
    SoundCoreParamValue, SoundCoreParameter,
};

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
/// for device in list_devices()? {
///     println!("{}: {}", device.id, device.description);
/// }
/// ```
pub fn list_devices() -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
    let endpoints = DeviceEnumerator::new()?.get_active_audio_endpoints()?;
    let mut result = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let id = endpoint.id()?;
        let span = debug_span!("Querying endpoint {id}...");
        let _span = span.enter();
        result.push(DeviceInfo {
            id,
            interface: endpoint.interface()?,
            description: endpoint.description()?,
        })
    }
    Ok(result)
}

fn get_endpoint(device_id: Option<&OsStr>) -> windows::core::Result<Endpoint> {
    let enumerator = DeviceEnumerator::new()?;
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
/// println!("{:?}", dump(None)?);
/// ```
pub fn dump(device_id: Option<&OsStr>) -> Result<Configuration, Box<dyn Error>> {
    let endpoint = get_endpoint(device_id)?;

    let endpoint_output = EndpointConfiguration {
        volume: Some(endpoint.get_volume()?),
    };

    let id = endpoint.id()?;
    debug!("Found device {id}");
    let clsid = endpoint.clsid()?;
    debug!(
        "Found clsid {{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        clsid.data1,
        clsid.data2,
        clsid.data3,
        clsid.data4[0],
        clsid.data4[1],
        clsid.data4[2],
        clsid.data4[3],
        clsid.data4[4],
        clsid.data4[5],
        clsid.data4[6],
        clsid.data4[7]
    );
    let core = SoundCore::for_device(&clsid, &id)?;

    let mut context_output = IndexMap::new();
    for feature in core.features(0) {
        let feature = feature?;
        let feature_span = debug_span!("feature {id} {description}", id = feature.id, description = %feature.description);
        let _feature_span = feature_span.enter();

        let mut feature_output = IndexMap::new();
        for parameter in feature.parameters() {
            let parameter = parameter?;
            let parameter_span = debug_span!("parameter {id} {description}", id = parameter.id, description = %parameter.description);
            let _parameter_span = parameter_span.enter();
            debug!(
                "attributes: {attributes}",
                attributes = parameter.attributes
            );
            if let Some(size) = parameter.size {
                debug!("size: {size}");
            }
            // skip read-only parameters
            if parameter.attributes & 1 == 0 {
                match parameter.kind {
                    1 => {
                        let value = parameter.get();
                        debug!("value: {value:?}");
                        match value {
                            Err(err) => {
                                error!(error = %err, "Unable to get value");
                            }
                            Ok(SoundCoreParamValue::None) => {}
                            Ok(value) => {
                                feature_output.insert(parameter.description.clone(), value);
                            }
                        }
                    }
                    0 | 2 | 3 => {
                        let value = parameter.get();
                        debug!("minimum:    {min_value:?}", min_value = parameter.min_value);
                        debug!("maximum:    {max_value:?}", max_value = parameter.max_value);
                        debug!("step:       {step_size:?}", step_size = parameter.step_size);
                        debug!("value:      {value:?}");
                        match value {
                            Err(err) => {
                                error!(error = %err, "Unable to get value");
                            }
                            Ok(SoundCoreParamValue::None) => {}
                            Ok(value) => {
                                feature_output.insert(parameter.description.clone(), value);
                            }
                        }
                    }
                    5 => {}
                    _ => {
                        debug!("kind:      {kind}", kind = parameter.kind);
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
/// set(None, &configuration, true);
/// ```
pub fn set(
    device_id: Option<&OsStr>,
    configuration: &Configuration,
    mute: bool,
) -> Result<(), Box<dyn Error>> {
    let endpoint = get_endpoint(device_id)?;
    let mute_unmute = mute && !endpoint.get_mute()?;
    if mute_unmute {
        endpoint.set_mute(true)?;
    }
    let result = set_internal(configuration, &endpoint);
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
/// for event in watch(None) {
///     println!("{:?}", event);
/// }
/// ```
pub fn watch(device_id: Option<&OsStr>) -> Result<SoundCoreEventIterator, Box<dyn Error>> {
    let endpoint = get_endpoint(device_id)?;
    let id = endpoint.id()?;
    let clsid = endpoint.clsid()?;
    let core = SoundCore::for_device(&clsid, &id)?;

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
    type Item = windows::core::Result<SoundCoreOrVolumeEvent>;

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
    type Item = windows::core::Result<SoundCoreOrVolumeEvent>;

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
/// for event in watch_with_volume(None) {
///     println!("{:?}", event);
/// }
/// ```
pub fn watch_with_volume(
    device_id: Option<&OsStr>,
) -> Result<SoundCoreAndVolumeEventIterator, Box<dyn Error>> {
    let endpoint = get_endpoint(device_id)?;
    let id = endpoint.id()?;
    let clsid = endpoint.clsid()?;
    let core = SoundCore::for_device(&clsid, &id)?;

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

fn set_internal(configuration: &Configuration, endpoint: &Endpoint) -> Result<(), Box<dyn Error>> {
    if let Some(ref creative) = configuration.creative {
        let id = endpoint.id()?;
        debug!("Found device {id}");
        let clsid = endpoint.clsid()?;
        debug!(
            "Found clsid \
             {{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            clsid.data1,
            clsid.data2,
            clsid.data3,
            clsid.data4[0],
            clsid.data4[1],
            clsid.data4[2],
            clsid.data4[3],
            clsid.data4[4],
            clsid.data4[5],
            clsid.data4[6],
            clsid.data4[7]
        );
        let core = SoundCore::for_device(&clsid, &id)?;

        let mut unhandled_feature_names = BTreeSet::<&str>::new();
        for (key, _) in creative.iter() {
            unhandled_feature_names.insert(key);
        }

        for feature in core.features(0) {
            let feature = feature?;
            let feature_span =
                trace_span!("Looking for {feature} settings...", feature = %feature.description);
            let _feature_span = feature_span.enter();
            if let Some(feature_table) = creative.get(&feature.description) {
                unhandled_feature_names.remove(&feature.description[..]);
                let mut unhandled_parameter_names = BTreeSet::<&str>::new();
                for (key, _) in feature_table.iter() {
                    unhandled_parameter_names.insert(key);
                }

                for parameter in feature.parameters() {
                    let mut parameter = parameter?;
                    let parameter_span = trace_span!("Looking for {parameter} settings...", parameter = %parameter.description);
                    let _parameter_span = parameter_span.enter();
                    if let Some(value) = feature_table.get(&parameter.description) {
                        unhandled_parameter_names.remove(&parameter.description[..]);
                        let value = &coerce_soundcore(&feature, &parameter, value)?;
                        if let Err(error) = parameter.set(value) {
                            error!(
                                "Could not set parameter {feature}.{parameter}: {error}",
                                feature = feature.description,
                                parameter = parameter.description,
                            );
                        }
                    }
                }
                for unhandled in unhandled_parameter_names {
                    warn!(
                        "Could not find parameter {feature}.{parameter}",
                        feature = feature.description,
                        parameter = unhandled,
                    );
                }
            }
        }
        for unhandled in unhandled_feature_names {
            warn!("Could not find feature {feature}", feature = unhandled);
        }
    }
    if let Some(ref endpoint_config) = configuration.endpoint {
        if let Some(v) = endpoint_config.volume {
            endpoint.set_volume(v)?;
        }
    }
    Ok(())
}
