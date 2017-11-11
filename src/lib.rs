extern crate ole32;
extern crate regex;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate toml;
#[macro_use]
extern crate winapi;

mod com;
mod ctsndcr;
mod hresult;
mod media;
mod soundcore;
mod winapiext;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use slog::Logger;

use toml::value::{Table, Value};

use media::{Endpoint, get_default_endpoint};
use soundcore::{get_sound_core, SoundCoreFeature, SoundCoreParameter, SoundCoreParamValue};

pub use com::{initialize_com, uninitialize_com};
pub use hresult::{Win32Error, check};
pub use soundcore::SoundCoreError;

#[derive(Debug, Deserialize)]
pub struct EndpointConfiguration {
    pub volume: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub endpoint: Option<EndpointConfiguration>,
    pub creative: Option<BTreeMap<String, BTreeMap<String, Value>>>,
}

fn convert_from_soundcore(value: SoundCoreParamValue) -> Value {
    match value {
        SoundCoreParamValue::Float(f) => Value::Float(f as f64),
        SoundCoreParamValue::Bool(b) => Value::Boolean(b),
        SoundCoreParamValue::U32(u) => Value::Integer(u as i64),
        SoundCoreParamValue::I32(i) => Value::Integer(i as i64),
        _ => Value::String("unexpectedly got an unsupported type".to_owned()),
    }
}

pub fn dump(logger: &Logger) -> Result<Table, Box<Error>> {
    let mut output = Table::new();

    let endpoint = get_default_endpoint(logger)?;

    let mut endpoint_output = Table::new();
    endpoint_output.insert(
        "volume".to_owned(),
        Value::Float(endpoint.get_volume()? as f64),
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
    let core = get_sound_core(&clsid, &id, &logger)?;

    let mut context_output = Table::new();
    for feature in core.features(0) {
        debug!(logger, "{:08x} {}", feature.id, feature.description);

        let mut feature_output = Table::new();
        for parameter in feature.parameters() {
            debug!(logger, "  {} {}", parameter.id, parameter.description);
            debug!(logger, "    attributes: {}", parameter.attributes);
            if let Some(size) = parameter.size {
                debug!(logger, "    size:       {}", size);
            }
            match parameter.kind {
                1 => {
                    let value = parameter.get();
                    debug!(logger, "    value:      {:?}", value);
                    feature_output.insert(parameter.description, convert_from_soundcore(value));
                }
                0 | 2 | 3 => {
                    let value = parameter.get();
                    debug!(logger, "    minimum:    {:?}", parameter.min_value);
                    debug!(logger, "    maximum:    {:?}", parameter.max_value);
                    debug!(logger, "    step:       {:?}", parameter.step_size);
                    debug!(logger, "    value:      {:?}", value);
                    feature_output.insert(parameter.description, convert_from_soundcore(value));
                }
                5 => {}
                _ => {
                    debug!(logger, "     kind:      {}", parameter.kind);
                }
            }
        }
        context_output.insert(feature.description, Value::Table(feature_output));
    }
    output.insert("creative".to_owned(), Value::Table(context_output));

    Ok(output)
}

pub fn set(logger: &Logger, configuration: &Configuration) -> Result<(), Box<Error>> {
    let endpoint = get_default_endpoint(logger)?;
    let premuted = endpoint.get_mute()?;
    if !premuted {
        endpoint.set_mute(true)?;
    }
    let result = set_internal(logger, configuration, &endpoint);
    if !premuted {
        endpoint.set_mute(false)?;
    }

    result
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
            self.feature,
            self.parameter,
            self.expected,
            self.actual
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
            actual: match value {
                &Value::Float(_) => "float",
                &Value::Boolean(_) => "bool",
                &Value::Integer(i) if i < 0 => "int",
                &Value::Integer(_) => "int|uint",
                _ => "<unsupported>",
            }.to_owned(),
        }),
    }
}

fn set_internal(
    logger: &Logger,
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
        let core = get_sound_core(&clsid, &id, &logger)?;

        let mut unhandled_feature_names = BTreeSet::<&str>::new();
        for (key, _) in creative.iter() {
            unhandled_feature_names.insert(key);
        }

        for feature in core.features(0) {
            trace!(logger, "Looking for {} settings...", feature.description);
            if let Some(ref feature_table) = creative.get(&feature.description) {
                unhandled_feature_names.remove(&feature.description[..]);
                let mut unhandled_parameter_names = BTreeSet::<&str>::new();
                for (key, _) in feature_table.iter() {
                    unhandled_parameter_names.insert(key);
                }

                for parameter in feature.parameters() {
                    trace!(
                        logger,
                        "Looking for {}.{} settings...",
                        feature.description,
                        parameter.description
                    );
                    if let Some(value) = feature_table.get(&parameter.description) {
                        unhandled_parameter_names.remove(&parameter.description[..]);
                        parameter.set(&convert_to_soundcore(&feature, &parameter, value)?);
                    }
                }
                for unhandled in unhandled_parameter_names {
                    warn!(
                        logger,
                        "Could not find parameter {}.{}",
                        feature.description,
                        unhandled
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
