use std::mem;

use slog::Logger;

use winapi::shared::winerror::E_FAIL;

use com::ComObject;
use ctsndcr::{FeatureInfo, ISoundCore};
use hresult::{check, Win32Error};

use SoundCoreFeature;

/// Iterates over features of a device.
pub struct SoundCoreFeatureIterator {
    target: ComObject<ISoundCore>,
    logger: Logger,
    context: u32,
    index: u32,
}

impl SoundCoreFeatureIterator {
    pub(crate) fn new(target: ComObject<ISoundCore>, logger: Logger, context: u32) -> Self {
        Self {
            target,
            logger,
            context,
            index: 0,
        }
    }
}

impl Iterator for SoundCoreFeatureIterator {
    type Item = Result<SoundCoreFeature, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreFeature, Win32Error>> {
        unsafe {
            let mut info: FeatureInfo = mem::zeroed();
            trace!(
                self.logger,
                "Fetching feature .{}[{}]...",
                self.context,
                self.index
            );
            match check(
                self.target
                    .EnumFeatures(self.context, self.index, &mut info),
            ) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code: code @ _, .. }) if code == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            trace!(
                self.logger,
                "Got feature .{}[{}] = {:?}",
                self.context,
                self.index,
                info
            );
            self.index += 1;
            match info.feature_id {
                0 => None,
                _ => Some(Ok(SoundCoreFeature::new(
                    self.target.clone(),
                    self.logger.clone(),
                    self.context,
                    &info,
                ))),
            }
        }
    }
}
