#![warn(missing_docs)]

//! Provides a high-level API for controlling Creative sound devices.
//!
//! For a lower-level API, see [`media`](media) and [`soundcore`](soundcore).
//!
//! For an even-lower-level API, see [`mmdeviceapi`](../winapi/um/mmdeviceapi) and [`ctsndcr`](ctsndcr).

extern crate regex;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate toml;
#[macro_use]
extern crate winapi;

mod com;
pub mod ctsndcr;
mod hresult;
mod lazy;
pub mod media;
pub mod soundcore;
mod winapiext;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;

use slog::Logger;

use toml::value::{Table, Value};

use soundcore::{get_sound_core, SoundCoreFeature, SoundCoreParamValue, SoundCoreParameter};

pub use com::{initialize_com, uninitialize_com};
pub use hresult::{check, Win32Error};
pub use media::{DeviceEnumerator, Endpoint};
pub use soundcore::{SoundCoreError, SoundCoreEventIterator};

/// Describes the configuration of a media endpoint.
#[derive(Debug, Deserialize)]
pub struct EndpointConfiguration {
    /// The desired volume level, from 0.0 to 1.0
    pub volume: Option<f32>,
}

/// Describes a configuration to be applied.
#[derive(Debug, Deserialize)]
pub struct Configuration {
    /// Windows audio endpoint settings
    pub endpoint: Option<EndpointConfiguration>,
    /// Creative SoundBlaster settings
    pub creative: Option<BTreeMap<String, BTreeMap<String, Value>>>,
}

fn convert_from_soundcore(value: &SoundCoreParamValue) -> Value {
    match *value {
        SoundCoreParamValue::Float(f) => Value::Float(f64::from(f)),
        SoundCoreParamValue::Bool(b) => Value::Boolean(b),
        SoundCoreParamValue::U32(u) => Value::Integer(i64::from(u)),
        SoundCoreParamValue::I32(i) => Value::Integer(i64::from(i)),
        _ => Value::String("unexpectedly got an unsupported type".to_owned()),
    }
}

/// Describes a device that may be configurable.
#[derive(Serialize)]
pub struct DeviceInfo {
    id: String,
    interface: String,
    description: String,
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
pub fn list_devices(logger: Logger) -> Result<Vec<DeviceInfo>, Box<Error>> {
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
pub fn dump(logger: Logger, device_id: Option<&OsStr>) -> Result<Table, Box<Error>> {
    let mut output = Table::new();

    let endpoint = get_endpoint(logger.clone(), device_id)?;

    let mut endpoint_output = Table::new();
    endpoint_output.insert(
        "volume".to_owned(),
        Value::Float(f64::from(endpoint.get_volume()?)),
    );
    output.insert("endpoint".to_owned(), Value::Table(endpoint_output));

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
    let core = get_sound_core(&clsid, &id, logger.clone())?;

    let mut context_output = Table::new();
    for feature in core.features(0) {
        let feature = feature?;
        debug!(logger, "{:08x} {}", feature.id, feature.description);

        let mut feature_output = Table::new();
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
                                feature_output.insert(
                                    parameter.description.clone(),
                                    convert_from_soundcore(&value),
                                );
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
                                feature_output.insert(
                                    parameter.description.clone(),
                                    convert_from_soundcore(&value),
                                );
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
            context_output.insert(feature.description.clone(), Value::Table(feature_output));
        }
    }
    output.insert("creative".to_owned(), Value::Table(context_output));

    Ok(output)
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
    logger: Logger,
    device_id: Option<&OsStr>,
    configuration: &Configuration,
    mute: bool,
) -> Result<(), Box<Error>> {
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

/// Get the sequence of events for a device.
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
    logger: Logger,
    device_id: Option<&OsStr>,
) -> Result<SoundCoreEventIterator, Box<Error>> {
    let endpoint = get_endpoint(logger.clone(), device_id)?;
    let id = endpoint.id()?;
    let clsid = endpoint.clsid()?;
    let core = get_sound_core(&clsid, &id, logger.clone())?;

    Ok(core.events()?)
}

#[derive(Debug)]
struct UnsupportedValueError {
    feature: String,
    parameter: String,
    expected: String,
    actual: String,
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

    fn cause(&self) -> Option<&Error> {
        None
    }
}

fn convert_to_soundcore(
    feature: &SoundCoreFeature,
    parameter: &SoundCoreParameter,
    value: &Value,
) -> Result<SoundCoreParamValue, UnsupportedValueError> {
    match (value, parameter.kind) {
        (&Value::Float(f), 0) => Ok(SoundCoreParamValue::Float(f as f32)),
        (&Value::Boolean(b), 1) => Ok(SoundCoreParamValue::Bool(b)),
        (&Value::Integer(i), 2) if 0 <= i => Ok(SoundCoreParamValue::U32(i as u32)),
        (&Value::Integer(i), 3) => Ok(SoundCoreParamValue::I32(i as i32)),
        _ => Err(UnsupportedValueError {
            feature: feature.description.to_owned(),
            parameter: parameter.description.to_owned(),
            expected: match parameter.kind {
                0 => "float",
                1 => "bool",
                2 => "uint",
                3 => "int",
                _ => "<unsupported>",
            }.to_owned(),
            actual: match *value {
                Value::Float(_) => "float",
                Value::Boolean(_) => "bool",
                Value::Integer(i) if i < 0 => "int",
                Value::Integer(_) => "int|uint",
                _ => "<unsupported>",
            }.to_owned(),
        }),
    }
}

fn set_internal(
    logger: Logger,
    configuration: &Configuration,
    endpoint: &Endpoint,
) -> Result<(), Box<Error>> {
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
        let core = get_sound_core(&clsid, &id, logger.clone())?;

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
                        let value = &convert_to_soundcore(&feature, &parameter, value)?;
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
