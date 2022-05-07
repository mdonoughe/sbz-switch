use std::mem;

use tracing::trace_span;
use winapi::shared::winerror::E_FAIL;

use crate::com::ComObject;
use crate::ctsndcr::{FeatureInfo, ISoundCore};
use crate::hresult::{check, Win32Error};

use crate::SoundCoreFeature;

/// Iterates over features of a device.
pub struct SoundCoreFeatureIterator {
    target: ComObject<ISoundCore>,
    context: u32,
    index: u32,
}

impl SoundCoreFeatureIterator {
    pub(crate) fn new(target: ComObject<ISoundCore>, context: u32) -> Self {
        Self {
            target,
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
            let span = trace_span!("Fetching feature .{context}[{index}]", context = self.context, index = self.index);
            let _span = span.enter();
            match check(
                self.target
                    .EnumFeatures(self.context, self.index, &mut info),
            ) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code, .. }) if code == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            span.record("info", &tracing::field::debug(&info));
            self.index += 1;
            match info.feature_id {
                0 => None,
                _ => Some(Ok(SoundCoreFeature::new(
                    self.target.clone(),
                    self.context,
                    &info,
                ))),
            }
        }
    }
}
