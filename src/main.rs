#[macro_use]
extern crate clap;
#[macro_use]
extern crate slog;
extern crate sbz_switch;
extern crate sloggers;
extern crate toml;

use clap::{AppSettings, Arg, ArgMatches, SubCommand};

use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::mem;
use std::str::FromStr;

use slog::Logger;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;

use toml::value::Value;

use sbz_switch::{initialize_com, uninitialize_com, Configuration, EndpointConfiguration};

fn main() {
    let device_arg = Arg::with_name("device")
        .short("d")
        .long("device")
        .value_name("DEVICE_ID")
        .help("Specify the device to act on (get id from list-devices)");
    let matches = app_from_crate!()
        .setting(AppSettings::AllowNegativeNumbers)
        .subcommand(
            SubCommand::with_name("list-devices")
                .about("prints out the names and IDs of available devices"),
        )
        .subcommand(
            SubCommand::with_name("dump")
                .arg(device_arg.clone())
                .about("prints out the current configuration")
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
                .about("applies a saved configuration")
                .arg(device_arg.clone())
                .arg(
                    Arg::with_name("file")
                        .short("f")
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
                .about("sets specific parameters")
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
        .get_matches();

    if matches.subcommand_name().is_none() {
        println!("{}", matches.usage());
        return;
    }

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Debug);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();

    trace!(logger, "Initializing COM...");
    initialize_com().unwrap();
    trace!(logger, "Initialized");

    let result = match matches.subcommand() {
        ("list-devices", _) => list_devices(logger.clone()),
        ("dump", Some(sub_m)) => dump(logger.clone(), sub_m),
        ("apply", Some(sub_m)) => apply(logger.clone(), sub_m),
        ("set", Some(sub_m)) => set(logger.clone(), sub_m),
        _ => Ok(()),
    };

    trace!(logger, "Uninitializing COM...");
    uninitialize_com();
    result.unwrap();
    debug!(logger, "Completed successfully");
}

fn list_devices(logger: Logger) -> Result<(), Box<Error>> {
    let devices = sbz_switch::list_devices(logger)?;
    let text = toml::to_string_pretty(&devices)?;
    print!("{}", text);
    Ok(())
}

fn dump(logger: Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let table = sbz_switch::dump(logger, matches.value_of_os("device"))?;
    let text = toml::to_string_pretty(&table)?;
    let output = matches.value_of("output");
    match output {
        Some(name) => write!(File::create(name)?, "{}", text)?,
        _ => print!("{}", text),
    }
    Ok(())
}

fn apply(logger: Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let mut text = String::new();
    match matches.value_of("file") {
        Some(name) => BufReader::new(File::open(name)?).read_to_string(&mut text)?,
        None => io::stdin().read_to_string(&mut text)?,
    };
    let configuration: Configuration = toml::from_str(&text)?;
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
    Collator { iter: iter, f: f }
}

fn set(logger: Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let mut creative_table = BTreeMap::<String, BTreeMap<String, Value>>::new();

    for (feature, parameter, value) in collate_set_values(matches.values_of("bool"), |s| {
        bool::from_str(s).map(Value::Boolean)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(BTreeMap::<String, Value>::new)
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in collate_set_values(matches.values_of("float"), |s| {
        f64::from_str(s).map(Value::Float)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(BTreeMap::<String, Value>::new)
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in collate_set_values(matches.values_of("int"), |s| {
        i64::from_str(s).map(Value::Integer)
    }) {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(BTreeMap::<String, Value>::new)
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
