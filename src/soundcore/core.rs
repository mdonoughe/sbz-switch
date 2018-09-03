use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{self, NonNull};

use slog::Logger;

use winapi::shared::guiddef::GUID;
use winapi::um::combaseapi::{CoCreateInstance, CLSCTX_ALL};
use winapi::Interface;

use ctsndcr::{HardwareInfo, ICallback, IEventNotify, ISoundCore};
use hresult::{check, Win32Error};

use super::event::event_iterator;
use super::{SoundCoreError, SoundCoreEventIterator, SoundCoreFeatureIterator};

/// Provides control of Creative SoundBlaster features.
///
/// This is a wrapper around `ISoundCore`.
pub struct SoundCore {
    sound_core: NonNull<ISoundCore>,
    logger: Logger,
}

impl SoundCore {
    fn bind_hardware(&mut self, id: &str) -> Result<(), Win32Error> {
        trace!(self.logger, "Binding SoundCore to {}...", id);
        let mut buffer = [0; 260];
        for c in OsStr::new(id).encode_wide().enumerate() {
            buffer[c.0] = c.1;
        }
        let info = HardwareInfo {
            info_type: 0,
            info: buffer,
        };
        check(unsafe { self.sound_core.as_mut().BindHardware(&info) })?;
        Ok(())
    }
    /// Returns an iterator over the features exposed by a device.
    pub fn features(&self, context: u32) -> SoundCoreFeatureIterator {
        SoundCoreFeatureIterator::new(self.sound_core, self.logger.clone(), context)
    }
    /// Returns an iterator over events produced by the SoundCore API.
    ///
    /// This includes events such as the speaker configuration changing.
    ///
    /// Calling `next` will block the current thread until events are available.
    ///
    /// Events are only received during a call to `next`.
    /// Events are likely dropped if `next` is called infrequently.
    ///
    /// Event iterators with overlapping lifetimes may produce unexpected results
    /// because the SoundCore API for events does not seem to support registering
    /// multiple event handlers and then unregistering only one of them. Probably
    /// this is okay if done with multiple `SoundCore` instances.
    pub fn events(&self) -> Result<SoundCoreEventIterator, Win32Error> {
        unsafe {
            let mut event_notify: *mut IEventNotify = mem::uninitialized();
            check(self.sound_core.as_ref().QueryInterface(
                &IEventNotify::uuidof(),
                &mut event_notify as *mut *mut _ as *mut _,
            ))?;
            let (mut w32sink, iterator) = event_iterator(
                NonNull::new(event_notify).unwrap(),
                self.sound_core,
                self.logger.clone(),
            );
            let callback = ICallback::new(move |e| {
                // despite our ICallback belonging to STA COM,
                // and events only firing while the main thread is processing events,
                // this executes on a different plain win32 thread,
                // so we need to marshal back to the correct thread
                // and we can't use std :(
                w32sink.send(*e);
                Ok(())
            });
            let result = check((*event_notify).RegisterEventCallback(0xff, callback));
            (*callback).Release();
            result?;
            Ok(iterator)
        }
    }
}

impl Drop for SoundCore {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            trace!(self.logger, "Releasing SoundCore...");
            self.sound_core.as_mut().Release();
        }
    }
}

fn create_sound_core(clsid: &GUID, logger: Logger) -> Result<SoundCore, SoundCoreError> {
    unsafe {
        let mut sc: *mut ISoundCore = mem::uninitialized();
        check(CoCreateInstance(
            clsid,
            ptr::null_mut(),
            CLSCTX_ALL,
            &ISoundCore::uuidof(),
            &mut sc as *mut *mut ISoundCore as *mut _,
        ))?;
        Ok(SoundCore {
            sound_core: NonNull::new(sc).unwrap(),
            logger,
        })
    }
}

/// Gets a SoundCore wrapper for an instance of the specified CLSID, controlling the specified device ID.
pub fn get_sound_core(
    clsid: &GUID,
    device_id: &str,
    logger: Logger,
) -> Result<SoundCore, SoundCoreError> {
    let mut core = create_sound_core(clsid, logger)?;
    core.bind_hardware(device_id)?;
    Ok(core)
}
