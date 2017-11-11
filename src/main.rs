extern crate clap;
#[macro_use]
extern crate slog;
extern crate sloggers;
extern crate sbz_switch;
extern crate toml;

use clap::{Arg, ArgMatches, App, AppSettings, SubCommand};

use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::prelude::*;
use std::mem;
use std::str::FromStr;

use slog::Logger;
use sloggers::Build;
use sloggers::terminal::{TerminalLoggerBuilder, Destination};
use sloggers::types::Severity;

use toml::value::Value;

use sbz_switch::{Configuration, EndpointConfiguration, initialize_com, uninitialize_com};

fn main() {
    let matches = App::new("sbz-switch")
        .version("0.1")
        .about("Switches outputs on Creative Sound Blaster devices")
        .author("Matthew Donoughe <mdonoughe@gmail.com>")
        .setting(AppSettings::AllowNegativeNumbers)
        .subcommand(
            SubCommand::with_name("dump")
                .about("prints out the current configuration")
                .arg(
                    Arg::with_name("output")
                        .short("o")
                        .long("output")
                        .value_name("FILE")
                        .help("Saves the current settings to a file")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("apply")
                .about("applies a saved configuration")
                .arg(
                    Arg::with_name("file")
                        .short("f")
                        .value_name("FILE")
                        .help("Reads the settings from a file instead of stdin")
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("set")
                .about("sets specific parameters")
                .arg(
                    Arg::with_name("bool")
                        .short("b")
                        .help("Sets a boolean value")
                        .takes_value(true)
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "true|false"]),
                )
                .arg(
                    Arg::with_name("int")
                        .short("i")
                        .help("Sets an integer value")
                        .takes_value(true)
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::with_name("float")
                        .short("f")
                        .help("Sets a floating-point value")
                        .takes_value(true)
                        .multiple(true)
                        .number_of_values(3)
                        .value_names(&["FEATURE", "PARAMETER", "VALUE"]),
                )
                .arg(
                    Arg::with_name("volume")
                        .short("v")
                        .long("volume")
                        .value_name("VOLUME")
                        .help("Sets the volume, in percent")
                        .takes_value(true),
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
        ("dump", Some(sub_m)) => dump(&logger, sub_m),
        ("apply", Some(sub_m)) => apply(&logger, sub_m),
        ("set", Some(sub_m)) => set(&logger, sub_m),
        _ => Ok(()),
    };

    trace!(logger, "Uninitializing COM...");
    uninitialize_com();
    result.unwrap();
    debug!(logger, "Completed successfully");
}

fn dump(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let table = sbz_switch::dump(logger)?;
    let text = toml::to_string(&table)?;
    let output = matches.value_of("output");
    match output {
        Some(name) => write!(File::create(name)?, "{}", text)?,
        _ => print!("{}", text),
    }
    Ok(())
}

fn apply(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let mut text = String::new();
    match matches.value_of("file") {
        Some(name) => BufReader::new(File::open(name)?).read_to_string(&mut text)?,
        None => io::stdin().read_to_string(&mut text)?,
    };
    let configuration: Configuration = toml::from_str(&text)?;
    mem::drop(text);

    sbz_switch::set(logger, &configuration)
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
            Some(ref mut iter) => {
                match iter.next() {
                    Some(value) => {
                        let first = value;
                        let second = iter.next().unwrap();
                        let third = iter.next().unwrap();
                        let f = &mut self.f;
                        Some((first, second, f(third)))
                    }
                    None => None,
                }
            }
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.iter {
            Some(ref iter) => {
                match iter.size_hint() {
                    (l, Some(u)) => (l / 3, Some(u / 3)),
                    (l, None) => (l / 3, None),
                }
            }
            None => (0, Some(0)),
        }
    }
}

fn collate_set_values<I, F>(iter: Option<I>, f: F) -> Collator<I, F> {
    Collator { iter: iter, f: f }
}

fn set(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let mut creative_table = BTreeMap::<String, BTreeMap<String, Value>>::new();

    for (feature, parameter, value) in
        collate_set_values(matches.values_of("bool"), |s| {
            bool::from_str(s).map(|b| Value::Boolean(b))
        })
    {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(|| BTreeMap::<String, Value>::new())
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in
        collate_set_values(matches.values_of("float"), |s| {
            f64::from_str(s).map(|f| Value::Float(f))
        })
    {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(|| BTreeMap::<String, Value>::new())
            .insert(parameter.to_owned(), value?);
    }

    for (feature, parameter, value) in
        collate_set_values(matches.values_of("int"), |s| {
            i64::from_str(s).map(|i| Value::Integer(i))
        })
    {
        creative_table
            .entry(feature.to_owned())
            .or_insert_with(|| BTreeMap::<String, Value>::new())
            .insert(parameter.to_owned(), value?);
    }

    let configuration = Configuration {
        endpoint: Some(EndpointConfiguration {
            volume: matches.value_of("volume").map(|s| {
                f32::from_str(s).unwrap() / 100.0
            }),
        }),
        creative: Some(creative_table),
    };
    sbz_switch::set(logger, &configuration)
}
