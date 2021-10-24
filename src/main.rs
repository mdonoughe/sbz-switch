#[macro_use]
extern crate clap;
extern crate indexmap;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;
#[macro_use]
extern crate slog;
extern crate sbz_switch;
extern crate sloggers;
extern crate toml;

extern crate confy;

use clap::{AppSettings, Arg, ArgMatches, SubCommand};

use indexmap::IndexMap;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::iter::IntoIterator;
use std::mem;
use std::str::FromStr;

use slog::Logger;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;

use toml::value::Value;

use sbz_switch::soundcore::SoundCoreParamValue;
use sbz_switch::{Configuration, DeviceInfo, EndpointConfiguration};

mod quickswitch;
use crate::quickswitch::QuickSwitchConfig;

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let device_arg = Arg::with_name("device")
        .short("d")
        .long("device")
        .value_name("DEVICE_ID")
        .help("Specify the device to act on (get id from list-devices)");
    let format_arg = Arg::with_name("format")
        .short("f")
        .value_name("FORMAT")
        .possible_values(&["toml", "json", "yaml"])
        .default_value("toml");
    let input_format_arg = format_arg.clone().help("Select the input format");
    let output_format_arg = format_arg.clone().help("Select the output format");
    let matches = app_from_crate!()
        .setting(AppSettings::AllowNegativeNumbers)
        .subcommand(
            SubCommand::with_name("list-devices")
                .about("Prints out the names and IDs of available devices")
                .arg(output_format_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name("dump")
                .about("Prints out the current configuration")
                .arg(device_arg.clone())
                .arg(output_format_arg.clone())
                .arg(
                    Arg::with_name("output")
                        .short("o")
                        .long("output")
                        .value_name("FILE")
                        .help("Saves the current settings to a file"),
                ),
        )
        .subcommand(
            SubCommand::with_name("apply")
                .about("Applies a saved configuration")
                .arg(device_arg.clone())
                .arg(input_format_arg)
                .arg(
                    Arg::with_name("file")
                        .short("i")
                        .value_name("FILE")
                        .help("Reads the settings from a file instead of stdin"),
                )
                .arg(
                    Arg::with_name("mute")
                        .short("m")
                        .value_name("true|false")
                        .default_value("true")
                        .help("Temporarily mutes while changing parameters"),
                ),
        )
        .subcommand(
            SubCommand::with_name("set")
                .about("Sets specific parameters")
                .arg(device_arg.clone())
                .arg(
                    Arg::with_name("bool")
                        .short("b")
                        .help("Sets a boolean value")
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "true|false"]),
                )
                .arg(
                    Arg::with_name("int")
                        .short("i")
                        .help("Sets an integer value")
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::with_name("float")
                        .short("f")
                        .help("Sets a floating-point value")
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::with_name("volume")
                        .short("v")
                        .long("volume")
                        .value_name("VOLUME")
                        .help("Sets the volume, in percent"),
                )
                .arg(
                    Arg::with_name("mute")
                        .short("m")
                        .value_name("true|false")
                        .default_value("true")
                        .help("Temporarily mutes while changing parameters"),
                ),
        )
        .subcommand(
            SubCommand::with_name("watch")
                .about("Watches for events")
                .arg(device_arg.clone())
                .arg(output_format_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name("switch")
                .about("Switches from Headphone to Speakers or vice versa")
                .arg(device_arg.clone()),
        )
        .get_matches();

    if matches.subcommand_name().is_none() {
        println!("{}", matches.usage());
        return 1;
    }

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Debug);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();

    let result = match matches.subcommand() {
        ("list-devices", Some(sub_m)) => list_devices(&logger, sub_m),
        ("dump", Some(sub_m)) => dump(&logger, sub_m),
        ("apply", Some(sub_m)) => apply(&logger, sub_m),
        ("set", Some(sub_m)) => set(&logger, sub_m),
        ("watch", Some(sub_m)) => watch(&logger, sub_m),
        ("switch", Some(sub_m)) => switch(&logger, sub_m),
        _ => unreachable!(),
    };

    match result {
        Ok(()) => {
            debug!(logger, "Completed successfully");
            0
        }
        Err(error) => {
            crit!(logger, "Unexpected error: {}", error);
            1
        }
    }
}

// Switches to Device 0 (Headphones) and sets System volume to 8 // Values can be overriden via quickswitch.conf
fn sth(logger: &Logger, dev: Option<&OsStr>) -> Result<(), Box<dyn Error>> {
    let conf: QuickSwitchConfig = confy::load_path("./quickswitch.conf")?;
    let mut creative_table = IndexMap::<String, IndexMap<String, SoundCoreParamValue>>::new();
    creative_table
        .entry("Device Control".to_string())
        .or_insert_with(IndexMap::<String, SoundCoreParamValue>::new)
        .insert(
            "SelectOutput".to_string(),
            sbz_switch::soundcore::SoundCoreParamValue::U32(conf.headphone_dev_id),
        );

    let config = Configuration {
        endpoint: Some(EndpointConfiguration {
            volume: Some(conf.headphone_vol / 100.0),
        }),
        creative: Some(creative_table),
    };
    sbz_switch::set(logger, dev, &config, conf.mute)
}
// Switches to device 1 (Speakers) and sets System Volume to 100 // Values can be overriden via quickswitch.conf
fn sts(logger: &Logger, dev: Option<&OsStr>) -> Result<(), Box<dyn Error>> {
    let conf: QuickSwitchConfig = confy::load_path("./quickswitch.conf")?;
    let mut creative_table = IndexMap::<String, IndexMap<String, SoundCoreParamValue>>::new();
    creative_table
        .entry("Device Control".to_string())
        .or_insert_with(IndexMap::<String, SoundCoreParamValue>::new)
        .insert(
            "SelectOutput".to_string(),
            sbz_switch::soundcore::SoundCoreParamValue::U32(conf.speaker_dev_id),
        );

    let config = Configuration {
        endpoint: Some(EndpointConfiguration {
            volume: Some(conf.speaker_vol / 100.0),
        }),
        creative: Some(creative_table),
    };
    sbz_switch::set(logger, dev, &config, conf.mute)
}

// This Function Switches The Output from Headphones to Speakers or Vice Versa while Simultaneously setting The appropriate Volume Levels
fn switch(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    // reuse the already existing functions to retrieve device status
    let table = sbz_switch::dump(logger, matches.value_of_os("device"))?;
    let devctrl = table.creative;
    let mut device: Option<u32> = None;
    match devctrl {
        Some(itable) => {
            // Get Device Status
            let activedevice = &itable["Device Control"];
            for a in activedevice {
                match (a.0.as_str(), a.1) {
                    ("SelectOutput", SoundCoreParamValue::U32(val)) => {
                        device = Some(*val);
                    }
                    _ => (),
                };
            }
        }
        _ => panic!("Device Error"),
    }
    println!("Dev: {}", device.unwrap());
    match device {
        Some(num) => {
            match num {
                // Headphones are Active Switch to speakers
                0 => {
                    println!("Headphones Detected!");
                    return sts(logger, matches.value_of_os("device"));
                }
                // Speakers are Active Switch to Headphones
                1 => {
                    println!("Speakers Detected!");
                    return sth(logger, matches.value_of_os("device"));
                }
                // Something else Happened and will be ignored
                _ => (),
            }
        }
        None => panic!("Output Missing Error"),
    }
    Ok(())
}

#[derive(Debug)]
enum FormatError {
    TomlRead(toml::de::Error),
    TomlWrite(toml::ser::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    ValueError(&'static str),
    ExpectedObject(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FormatError::TomlRead(error) => error.fmt(f),
            FormatError::TomlWrite(error) => error.fmt(f),
            FormatError::Json(error) => error.fmt(f),
            FormatError::Yaml(error) => error.fmt(f),
            FormatError::ValueError(error) => write!(f, "unsupported value of type {}", error),
            FormatError::ExpectedObject(name) => write!(f, "expected {} to be an object", name),
        }
    }
}

impl Error for FormatError {
    fn cause(&self) -> Option<&dyn Error> {
        match &self {
            FormatError::TomlRead(error) => Some(error),
            FormatError::TomlWrite(error) => Some(error),
            FormatError::Json(error) => Some(error),
            FormatError::Yaml(error) => Some(error),
            FormatError::ValueError(_) => None,
            FormatError::ExpectedObject(_) => None,
        }
    }
}

fn format_configuration(
    value: &Configuration,
    matches: &ArgMatches,
) -> Result<String, FormatError> {
    match matches.value_of("format").unwrap() {
        "toml" => {
            let value: SerdeConfiguration<BTreeMap<String, BTreeMap<String, Value>>> =
                SerdeConfiguration {
                    endpoint: value.endpoint.as_ref().map(From::from),
                    creative: value.creative.as_ref().map(|creative| {
                        creative
                            .into_iter()
                            .map(|(feature, params)| {
                                (
                                    feature.clone(),
                                    params
                                        .into_iter()
                                        .map(|(key, value)| (key.clone(), Value::from_param(value)))
                                        .collect(),
                                )
                            })
                            .collect()
                    }),
                };
            toml::to_string_pretty(&value).map_err(FormatError::TomlWrite)
        }
        "json" => {
            let value: SerdeConfiguration<serde_json::Map<String, serde_json::Value>> =
                SerdeConfiguration {
                    endpoint: value.endpoint.as_ref().map(From::from),
                    creative: value.creative.as_ref().map(|creative| {
                        creative
                            .into_iter()
                            .map(|(feature, params)| {
                                (
                                    feature.to_string(),
                                    serde_json::Value::Object(
                                        params
                                            .into_iter()
                                            .map(|(key, value)| {
                                                (
                                                    key.to_string(),
                                                    serde_json::Value::from_param(value),
                                                )
                                            })
                                            .collect(),
                                    ),
                                )
                            })
                            .collect()
                    }),
                };
            serde_json::to_string_pretty(&value).map_err(FormatError::Json)
        }
        "yaml" => {
            let value: SerdeConfiguration<serde_yaml::Mapping> = SerdeConfiguration {
                endpoint: value.endpoint.as_ref().map(From::from),
                creative: value.creative.as_ref().map(|creative| {
                    creative
                        .into_iter()
                        .map(|(feature, params)| {
                            (
                                serde_yaml::Value::String(feature.to_string()),
                                serde_yaml::Value::Mapping(
                                    params
                                        .into_iter()
                                        .map(|(key, value)| {
                                            (
                                                serde_yaml::Value::String(key.to_string()),
                                                serde_yaml::Value::from_param(value),
                                            )
                                        })
                                        .collect(),
                                ),
                            )
                        })
                        .collect()
                }),
            };
            serde_yaml::to_string(&value).map_err(FormatError::Yaml)
        }
        _ => unreachable!(),
    }
}

fn jobject_into_map(
    value: serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, ()> {
    match value {
        serde_json::Value::Object(o) => Ok(o),
        _ => Err(()),
    }
}

fn ystring_into_string(value: serde_yaml::Value) -> Result<String, ()> {
    match value {
        serde_yaml::Value::String(s) => Ok(s),
        _ => Err(()),
    }
}

fn yobject_into_map(value: serde_yaml::Value) -> Result<serde_yaml::Mapping, ()> {
    match value {
        serde_yaml::Value::Mapping(o) => Ok(o),
        _ => Err(()),
    }
}

fn transpose<T, E>(value: Option<Result<T, E>>) -> Result<Option<T>, E> {
    match value {
        Some(Ok(value)) => Ok(Some(value)),
        Some(Err(error)) => Err(error),
        None => Ok(None),
    }
}

fn unformat_configuration(value: &str, matches: &ArgMatches) -> Result<Configuration, FormatError> {
    Ok(match matches.value_of("format").unwrap() {
        "toml" => {
            let value: SerdeConfiguration<BTreeMap<String, BTreeMap<String, Value>>> =
                toml::from_str(&value).map_err(FormatError::TomlRead)?;
            Configuration {
                endpoint: value.endpoint.map(Into::into),
                creative: transpose(value.creative.map(|creative| {
                    Ok({
                        creative
                            .into_iter()
                            .map(|(feature, params)| {
                                Ok({
                                    (
                                        feature,
                                        params
                                            .into_iter()
                                            .map(|(key, value)| {
                                                Ok((
                                                    key,
                                                    Value::try_into_param(value)
                                                        .map_err(FormatError::ValueError)?,
                                                ))
                                            })
                                            .collect::<Result<_, _>>()?,
                                    )
                                })
                            })
                            .collect::<Result<_, _>>()?
                    })
                }))?,
            }
        }
        "json" => {
            let value: SerdeConfiguration<serde_json::Map<String, serde_json::Value>> =
                serde_json::from_str(&value).map_err(FormatError::Json)?;
            Configuration {
                endpoint: value.endpoint.map(Into::into),
                creative: transpose(value.creative.map(|creative| {
                    Ok({
                        creative
                            .into_iter()
                            .map(|(feature, params)| {
                                Ok({
                                    let params = match jobject_into_map(params) {
                                        Ok(params) => params,
                                        Err(_) => return Err(FormatError::ExpectedObject(feature)),
                                    };
                                    (
                                        feature,
                                        params
                                            .into_iter()
                                            .map(|(key, value)| {
                                                Ok((
                                                    key,
                                                    serde_json::Value::try_into_param(value)
                                                        .map_err(FormatError::ValueError)?,
                                                ))
                                            })
                                            .collect::<Result<_, _>>()?,
                                    )
                                })
                            })
                            .collect::<Result<_, _>>()?
                    })
                }))?,
            }
        }
        "yaml" => {
            let value: SerdeConfiguration<serde_yaml::Mapping> =
                serde_yaml::from_str(&value).map_err(FormatError::Yaml)?;
            Configuration {
                endpoint: value.endpoint.map(Into::into),
                creative: transpose(value.creative.map(|creative| {
                    Ok({
                        creative
                            .into_iter()
                            .map(|(feature, params)| {
                                Ok({
                                    let feature = ystring_into_string(feature)
                                        .expect("yaml property name was not a string");
                                    let params = match yobject_into_map(params) {
                                        Ok(params) => params,
                                        Err(_) => return Err(FormatError::ExpectedObject(feature)),
                                    };
                                    (
                                        feature,
                                        params
                                            .into_iter()
                                            .map(|(key, value)| {
                                                Ok((
                                                    ystring_into_string(key).expect(
                                                        "yaml property name was not a string",
                                                    ),
                                                    serde_yaml::Value::try_into_param(value)
                                                        .map_err(FormatError::ValueError)?,
                                                ))
                                            })
                                            .collect::<Result<_, _>>()?,
                                    )
                                })
                            })
                            .collect::<Result<_, _>>()?
                    })
                }))?,
            }
        }
        _ => unreachable!(),
    })
}

#[derive(Deserialize, Serialize)]
pub struct SerdeEndpointConfiguration {
    pub volume: Option<f32>,
}

impl From<&EndpointConfiguration> for SerdeEndpointConfiguration {
    fn from(value: &EndpointConfiguration) -> Self {
        Self {
            volume: value.volume,
        }
    }
}

impl From<SerdeEndpointConfiguration> for EndpointConfiguration {
    fn from(value: SerdeEndpointConfiguration) -> Self {
        Self {
            volume: value.volume,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct SerdeConfiguration<TOuter> {
    endpoint: Option<SerdeEndpointConfiguration>,
    creative: Option<TOuter>,
}

trait ParamConvert {
    fn try_into_param(value: Self) -> Result<SoundCoreParamValue, &'static str>;
    fn from_param(value: &SoundCoreParamValue) -> Self;
}

impl ParamConvert for toml::Value {
    fn try_into_param(value: Self) -> Result<SoundCoreParamValue, &'static str> {
        match value {
            Value::Float(f) => Ok(SoundCoreParamValue::Float(f as f32)),
            Value::Boolean(b) => Ok(SoundCoreParamValue::Bool(b)),
            Value::Integer(i)
                if i < i64::from(i32::min_value()) || i64::from(u32::max_value()) < i =>
            {
                Err("Large integer")
            }
            Value::Integer(i) if i64::from(i32::max_value()) <= i => {
                Ok(SoundCoreParamValue::U32(i as u32))
            }
            Value::Integer(i) => Ok(SoundCoreParamValue::I32(i as i32)),
            Value::Array(_) => Err("Array"),
            Value::Datetime(_) => Err("Datetime"),
            Value::Table(_) => Err("Table"),
            Value::String(_) => Err("String"),
        }
    }
    fn from_param(value: &SoundCoreParamValue) -> Self {
        match value {
            SoundCoreParamValue::Float(f) => Value::Float((*f).into()),
            SoundCoreParamValue::Bool(b) => Value::Boolean(*b),
            SoundCoreParamValue::U32(i) => Value::Integer(i64::from(*i)),
            SoundCoreParamValue::I32(i) => Value::Integer(i64::from(*i)),
            _ => Value::String("<unsupported>".to_string()),
        }
    }
}

impl ParamConvert for serde_json::Value {
    fn try_into_param(value: Self) -> Result<SoundCoreParamValue, &'static str> {
        match value {
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(n) if n < i64::from(i32::min_value()) => Err("Large integer"),
                Some(n) if n <= i64::from(i32::max_value()) => {
                    Ok(SoundCoreParamValue::I32(n as i32))
                }
                Some(n) if n <= i64::from(u32::max_value()) => {
                    Ok(SoundCoreParamValue::U32(n as u32))
                }
                Some(_) => Err("Large integer"),
                None => Ok(SoundCoreParamValue::Float(n.as_f64().unwrap() as f32)),
            },
            serde_json::Value::Bool(b) => Ok(SoundCoreParamValue::Bool(b)),
            serde_json::Value::Array(_) => Err("Array"),
            serde_json::Value::Object(_) => Err("Object"),
            serde_json::Value::String(_) => Err("String"),
            serde_json::Value::Null => Err("Null"),
        }
    }
    fn from_param(value: &SoundCoreParamValue) -> Self {
        match value {
            SoundCoreParamValue::Float(f) => serde_json::Value::from(*f),
            SoundCoreParamValue::Bool(b) => serde_json::Value::from(*b),
            SoundCoreParamValue::U32(i) => serde_json::Value::from(*i),
            SoundCoreParamValue::I32(i) => serde_json::Value::from(*i),
            _ => serde_json::Value::String("<unsupported>".to_string()),
        }
    }
}

impl ParamConvert for serde_yaml::Value {
    fn try_into_param(value: Self) -> Result<SoundCoreParamValue, &'static str> {
        match value {
            serde_yaml::Value::Number(n) => match n.as_i64() {
                Some(n) if n < i64::from(i32::min_value()) => Err("Large integer"),
                Some(n) if n <= i64::from(i32::max_value()) => {
                    Ok(SoundCoreParamValue::I32(n as i32))
                }
                Some(n) if n <= i64::from(u32::max_value()) => {
                    Ok(SoundCoreParamValue::U32(n as u32))
                }
                Some(_) => Err("Large integer"),
                None => Ok(SoundCoreParamValue::Float(n.as_f64().unwrap() as f32)),
            },
            serde_yaml::Value::Bool(b) => Ok(SoundCoreParamValue::Bool(b)),
            serde_yaml::Value::Sequence(_) => Err("Sequence"),
            serde_yaml::Value::Mapping(_) => Err("Mapping"),
            serde_yaml::Value::String(_) => Err("String"),
            serde_yaml::Value::Null => Err("Null"),
        }
    }
    fn from_param(value: &SoundCoreParamValue) -> Self {
        match value {
            SoundCoreParamValue::Float(f) => serde_yaml::Value::from(*f),
            SoundCoreParamValue::Bool(b) => serde_yaml::Value::from(*b),
            SoundCoreParamValue::U32(i) => serde_yaml::Value::from(*i),
            SoundCoreParamValue::I32(i) => serde_yaml::Value::from(*i),
            _ => serde_yaml::Value::String("<unsupported>".to_string()),
        }
    }
}

#[derive(Serialize)]
struct SerializableDeviceInfo {
    id: String,
    interface: String,
    description: String,
}

impl From<DeviceInfo> for SerializableDeviceInfo {
    fn from(value: DeviceInfo) -> SerializableDeviceInfo {
        SerializableDeviceInfo {
            id: value.id,
            interface: value.interface,
            description: value.description,
        }
    }
}

fn list_devices(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let devices: Vec<_> = sbz_switch::list_devices(logger)?
        .into_iter()
        .map(SerializableDeviceInfo::from)
        .collect();
    let text = match matches.value_of("format").unwrap() {
        "toml" => toml::to_string_pretty(&devices).map_err(FormatError::TomlWrite)?,
        "json" => serde_json::to_string_pretty(&devices).map_err(FormatError::Json)?,
        "yaml" => serde_yaml::to_string(&devices).map_err(FormatError::Yaml)?,
        _ => unreachable!(),
    };
    print!("{}", text);
    Ok(())
}

fn dump(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let table = sbz_switch::dump(logger, matches.value_of_os("device"))?;
    let text = format_configuration(&table, matches)?;
    let output = matches.value_of("output");
    match output {
        Some(name) => write!(File::create(name)?, "{}", text)?,
        _ => print!("{}", text),
    }
    Ok(())
}

fn apply(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    match matches.value_of("file") {
        Some(name) => BufReader::new(File::open(name)?).read_to_string(&mut text)?,
        None => io::stdin().read_to_string(&mut text)?,
    };

    let configuration: Configuration = unformat_configuration(&text, matches)?;
    mem::drop(text);

    let mute = value_t!(matches, "mute", bool)?;
    sbz_switch::set(logger, matches.value_of_os("device"), &configuration, mute)
}

struct Collator<I, F> {
    iter: Option<I>,
    f: F,
}

impl<B, I: Iterator, F> Iterator for Collator<I, F>
where
    F: FnMut(I::Item) -> B,
{
    type Item = (I::Item, I::Item, B);

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter {
            Some(ref mut iter) => match iter.next() {
                Some(value) => {
                    let first = value;
                    let second = iter.next().unwrap();
                    let third = iter.next().unwrap();
                    let f = &mut self.f;
                    Some((first, second, f(third)))
                }
                None => None,
            },
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.iter {
            Some(ref iter) => match iter.size_hint() {
                (l, Some(u)) => (l / 3, Some(u / 3)),
                (l, None) => (l / 3, None),
            },
            None => (0, Some(0)),
        }
    }
}

fn collate_set_values<I, F>(iter: Option<I>, f: F) -> Collator<I, F> {
    Collator { iter, f }
}

fn set(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let mut creative_table = IndexMap::<String, IndexMap<String, SoundCoreParamValue>>::new();

    for (feature, parameter, value) in collate_set_values(matches.values_of("bool"), |s| {
        bool::from_str(s).map(SoundCoreParamValue::Bool)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(IndexMap::<String, SoundCoreParamValue>::new)
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in collate_set_values(matches.values_of("float"), |s| {
        f32::from_str(s).map(SoundCoreParamValue::Float)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(IndexMap::<String, SoundCoreParamValue>::new)
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in collate_set_values(matches.values_of("int"), |s| {
        i32::from_str(s).map(SoundCoreParamValue::I32)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(IndexMap::<String, SoundCoreParamValue>::new)
            .insert(parameter.to_owned(), value?);
    }

    let configuration = Configuration {
        endpoint: Some(EndpointConfiguration {
            volume: matches
                .value_of("volume")
                .map(|s| f32::from_str(s).unwrap() / 100.0),
        }),
        creative: Some(creative_table),
    };

    let mute = value_t!(matches, "mute", bool)?;
    sbz_switch::set(logger, matches.value_of_os("device"), &configuration, mute)
}

fn watch(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    for event in sbz_switch::watch_with_volume(logger, matches.value_of_os("device"))? {
        println!("{:?}", event);
    }
    Ok(())
}
