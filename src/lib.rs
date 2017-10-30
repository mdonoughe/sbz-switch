extern crate ole32;
extern crate regex;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate winapi;

mod com;
mod ctsndcr;
mod hresult;
mod media;
mod soundcore;
mod winapiext;

use std::error;

use slog::Logger;

use media::get_default_endpoint;
use soundcore::get_sound_core;

pub use com::{initialize_com, uninitialize_com};
pub use hresult::{Win32Error, check};
pub use soundcore::SoundCoreError;

pub fn switch(
    logger: &Logger,
    speakers: u32,
    volume: Option<f32>,
) -> Result<(), Box<error::Error>> {
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
