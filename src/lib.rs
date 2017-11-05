extern crate ole32;
extern crate regex;
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

use std::error::Error;

use slog::Logger;

use toml::value::{Table, Value};

use media::get_default_endpoint;
use soundcore::{get_sound_core, SoundCoreParamValue};

pub use com::{initialize_com, uninitialize_com};
pub use hresult::{Win32Error, check};
pub use soundcore::SoundCoreError;

fn convert_value(value: SoundCoreParamValue) -> Value {
    match value {
        SoundCoreParamValue::Float(f) => Value::Float(f as f64),
        SoundCoreParamValue::Bool(b) => Value::Boolean(b),
        SoundCoreParamValue::U32(u) => Value::Integer(u as i64),
        SoundCoreParamValue::I32(i) => Value::Integer(i as i64),
        _ => Value::String("unexpectedly got an unsupported type".to_owned()),
    }
}

pub fn dump(
    logger: &Logger,
) -> Result<Table, Box<Error>> {
    let mut output = Table::new();

    let endpoint = get_default_endpoint(logger)?;

    let mut endpoint_output = Table::new();
    endpoint_output.insert("volume".to_owned(), Value::Float(endpoint.get_volume()? as f64));
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
                    feature_output.insert(parameter.description, convert_value(value));
                },
                0 | 2 | 3 => {
                    let value = parameter.get();
                    debug!(logger, "    minimum:    {:?}", parameter.min_value);
                    debug!(logger, "    maximum:    {:?}", parameter.max_value);
                    debug!(logger, "    step:       {:?}", parameter.step_size);
                    debug!(logger, "    value:      {:?}", value);
                    feature_output.insert(parameter.description, convert_value(value));
                },
                5 => {},
                _ => {
                    debug!(logger, "     kind:      {}", parameter.kind);
                },
            }
        }
        context_output.insert(feature.description, Value::Table(feature_output));
    }
    output.insert("creative".to_owned(), Value::Table(context_output));

    Ok(output)
}

pub fn switch(
    logger: &Logger,
    speakers: u32,
    volume: Option<f32>,
) -> Result<(), Box<Error>> {
    let endpoint = get_default_endpoint(logger)?;

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
