extern crate clap;
#[macro_use]
extern crate slog;
extern crate sloggers;
extern crate sbz_switch;
extern crate toml;

use clap::{Arg, ArgMatches, App, SubCommand};

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

use sbz_switch::{initialize_com, uninitialize_com};

fn main() {
    let matches = App::new("sbz-switch")
        .version("0.1")
        .about("Switches outputs on Creative Sound Blaster devices")
        .author("Matthew Donoughe <mdonoughe@gmail.com>")
        .subcommand(SubCommand::with_name("dump")
            .about("prints out the current configuration")
            .arg(Arg::with_name("output")
                .short("o")
                .long("output")
                .value_name("FILE")
                .help("Saves the current settings to a file")
                .takes_value(true)))
        .subcommand(SubCommand::with_name("set")
            .about("sets the current configuration")
            .arg(Arg::with_name("file")
                .short("f")
                .value_name("FILE")
                .help("File containing the settings to apply")
                .takes_value(true)
                .required(true)))
        .subcommand(SubCommand::with_name("switch")
            .arg(
                Arg::with_name("speakers")
                    .short("s")
                    .long("speakers")
                    .value_name("CONFIG")
                    .help("\"3003\" for stereo speakers, \"80000000\" for headphones.")
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name("volume")
                    .short("v")
                    .long("volume")
                    .value_name("VOLUME")
                    .help("The target volume level, in percent")
                    .takes_value(true),
            ))
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
        ("set", Some(sub_m)) => set(&logger, sub_m),
        ("switch", Some(sub_m)) => switch(&logger, sub_m),
        _ => Ok(())
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
        Some(name) => {
            write!(File::create(name)?, "{}", text)?;
        },
        _ => {
            print!("{}", text);
        }
    }
    Ok(())
}

fn set(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let mut text = String::new();
    let name = matches.value_of("file").unwrap();
    match name {
        "-" => io::stdin().read_to_string(&mut text)?,
        _ => BufReader::new(File::open(name)?).read_to_string(&mut text)?
    };
    let table = toml::from_str(&text)?;
    mem::drop(text);

    sbz_switch::set(logger, &table)
}

fn switch(logger: &Logger, matches: &ArgMatches) -> Result<(), Box<Error>> {
    let speakers = u32::from_str_radix(matches.value_of("speakers").unwrap(), 16).unwrap();
    let volume = matches.value_of("volume").map(|s| {
        f32::from_str(s).unwrap() / 100.0
    });
    sbz_switch::switch(logger, speakers, volume)
}
