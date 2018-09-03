use std::mem;

use slog::Logger;

use winapi::shared::winerror::E_FAIL;

use com::ComObject;
use ctsndcr::{ISoundCore, ParamInfo};
use hresult::{check, Win32Error};

use SoundCoreParameter;

/// Iterates over the parameters of a feature.
pub struct SoundCoreParameterIterator {
    target: ComObject<ISoundCore>,
    logger: Logger,
    context: u32,
    feature_id: u32,
    feature_description: String,
    index: u32,
}

impl SoundCoreParameterIterator {
    pub(crate) fn new(
        target: ComObject<ISoundCore>,
        logger: Logger,
        context: u32,
        feature_id: u32,
        feature_description: String,
    ) -> Self {
        let result = Self {
            target,
            logger,
            context,
            feature_id,
            feature_description,
            index: 0,
        };
        result
    }
}

impl Iterator for SoundCoreParameterIterator {
    type Item = Result<SoundCoreParameter, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreParameter, Win32Error>> {
        unsafe {
            let mut info: ParamInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching parameter .{}.{}[{}]...",
                self.context,
                self.feature_description,
                self.index
            );
            match check(self.target.EnumParams(
                self.context,
                self.index,
                self.feature_id,
                &mut info,
            )) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code: code @ _, .. }) if code == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            trace!(
                self.logger,
                "Got parameter .{}.{}[{}] = {:?}",
                self.context,
                self.feature_description,
                self.index,
                info
            );
            self.index += 1;
            match info.param.feature {
                0 => None,
                _ => Some(Ok(SoundCoreParameter::new(
                    self.target.clone(),
                    self.feature_description.clone(),
                    self.logger.clone(),
                    &info,
                ))),
            }
        }
    }
}
