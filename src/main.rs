#[macro_use]
extern crate serde_derive;

use clap::Command;
use clap::{Arg, ArgMatches};

use indexmap::IndexMap;
use tracing::{debug, error};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use windows::core::HSTRING;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::iter::IntoIterator;
use std::mem;
use std::str::FromStr;

use toml::value::Value;

use sbz_switch::soundcore::SoundCoreParamValue;
use sbz_switch::{Configuration, DeviceInfo, EndpointConfiguration};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let device_arg = Arg::new("device")
        .short('d')
        .long("device")
        .value_name("DEVICE_ID")
        .help("Specify the device to act on (get id from list-devices)");
    let format_arg = Arg::new("format")
        .short('f')
        .value_name("FORMAT")
        .possible_values(&["toml", "json", "yaml"])
        .default_value("toml");
    let input_format_arg = format_arg.clone().help("Select the input format");
    let output_format_arg = format_arg.clone().help("Select the output format");
    let matches = clap::command!()
        .allow_negative_numbers(true)
        .subcommand_required(true)
        .subcommand(
            Command::new("list-devices")
                .about("Prints out the names and IDs of available devices")
                .arg(output_format_arg.clone()),
        )
        .subcommand(
            Command::new("dump")
                .about("Prints out the current configuration")
                .arg(device_arg.clone())
                .arg(output_format_arg.clone())
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("FILE")
                        .help("Saves the current settings to a file"),
                ),
        )
        .subcommand(
            Command::new("apply")
                .about("Applies a saved configuration")
                .arg(device_arg.clone())
                .arg(input_format_arg)
                .arg(
                    Arg::new("file")
                        .short('i')
                        .value_name("FILE")
                        .help("Reads the settings from a file instead of stdin"),
                )
                .arg(
                    Arg::new("mute")
                        .short('m')
                        .value_name("true|false")
                        .default_value("true")
                        .help("Temporarily mutes while changing parameters"),
                ),
        )
        .subcommand(
            Command::new("set")
                .about("Sets specific parameters")
                .arg(device_arg.clone())
                .arg(
                    Arg::new("bool")
                        .short('b')
                        .help("Sets a boolean value")
                        .multiple_occurrences(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "true|false"]),
                )
                .arg(
                    Arg::new("int")
                        .short('i')
                        .help("Sets an integer value")
                        .multiple_occurrences(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::new("float")
                        .short('f')
                        .help("Sets a floating-point value")
                        .multiple_occurrences(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::new("volume")
                        .short('v')
                        .long("volume")
                        .value_name("VOLUME")
                        .help("Sets the volume, in percent"),
                )
                .arg(
                    Arg::new("mute")
                        .short('m')
                        .value_name("true|false")
                        .default_value("true")
                        .help("Temporarily mutes while changing parameters"),
                ),
        )
        .subcommand(
            Command::new("watch")
                .about("Watches for events")
                .arg(device_arg.clone())
                .arg(output_format_arg.clone()),
        )
        .get_matches();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .init();

    let result = match matches.subcommand().unwrap() {
        ("list-devices", sub_m) => list_devices(sub_m),
        ("dump", sub_m) => dump(sub_m),
        ("apply", sub_m) => apply(sub_m),
        ("set", sub_m) => set(sub_m),
        ("watch", sub_m) => watch(sub_m),
        _ => unreachable!(),
    };

    match result {
        Ok(()) => {
            debug!("Completed successfully");
            0
        }
        Err(error) => {
            error!(error = %error, "Unexpected error");
            1
        }
    }
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
                toml::from_str(value).map_err(FormatError::TomlRead)?;
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
                serde_json::from_str(value).map_err(FormatError::Json)?;
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
                serde_yaml::from_str(value).map_err(FormatError::Yaml)?;
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

fn list_devices(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let devices: Vec<_> = sbz_switch::list_devices()?
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

fn dump(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let table = sbz_switch::dump(matches.value_of_os("device").map(HSTRING::from).as_ref())?;
    let text = format_configuration(&table, matches)?;
    let output = matches.value_of("output");
    match output {
        Some(name) => write!(File::create(name)?, "{}", text)?,
        _ => print!("{}", text),
    }
    Ok(())
}

fn apply(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    match matches.value_of("file") {
        Some(name) => BufReader::new(File::open(name)?).read_to_string(&mut text)?,
        None => io::stdin().read_to_string(&mut text)?,
    };

    let configuration: Configuration = unformat_configuration(&text, matches)?;
    mem::drop(text);

    let mute = matches.value_of_t("mute")?;
    sbz_switch::set(
        matches.value_of_os("device").map(HSTRING::from).as_ref(),
        &configuration,
        mute,
    )
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

fn set(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
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

    let mute = matches.value_of_t("mute")?;
    sbz_switch::set(
        matches.value_of_os("device").map(HSTRING::from).as_ref(),
        &configuration,
        mute,
    )
}

fn watch(matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    for event in
        sbz_switch::watch_with_volume(matches.value_of_os("device").map(HSTRING::from).as_ref())?
    {
        println!("{:?}", event);
    }
    Ok(())
}
