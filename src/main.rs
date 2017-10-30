extern crate clap;
#[macro_use]
extern crate slog;
extern crate sloggers;
extern crate sbz_switch;

use clap::{Arg, App};

use std::str::FromStr;

use sloggers::Build;
use sloggers::terminal::{TerminalLoggerBuilder, Destination};
use sloggers::types::Severity;

use sbz_switch::{initialize_com, switch, uninitialize_com};

fn main() {
    let matches = App::new("sbz-switch")
        .version("0.1")
        .about("Switches outputs on Creative Sound Blaster devices")
        .author("Matthew Donoughe <mdonoughe@gmail.com>")
        .arg(Arg::with_name("speakers")
            .short("s")
            .long("speakers")
            .value_name("CONFIG")
            .help("The speaker configuration to use. \"3003\" for stereo speakers, \"80000000\" for headphones.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("volume")
            .short("v")
            .long("volume")
            .value_name("VOLUME")
            .help("The target volume level, in percent.")
            .takes_value(true))
        .get_matches();

    let speakers = u32::from_str_radix(matches.value_of("speakers").unwrap(), 16).unwrap();
    let volume = matches.value_of("volume").map(|s| f32::from_str(s).unwrap() / 100.0);

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(Severity::Trace);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();

    trace!(logger, "Initializing COM...");
    initialize_com().unwrap();
    trace!(logger, "Initialized");
    let result = switch(&logger, speakers, volume);
    trace!(logger, "Uninitializing COM...");
    uninitialize_com();
    result.unwrap();
    debug!(logger, "Completed successfully");
}
