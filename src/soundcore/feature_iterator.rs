use std::mem::MaybeUninit;

use tracing::trace_span;
use windows::Win32::Foundation::E_FAIL;

use crate::com::ComObject;
use crate::ctsndcr::ISoundCore;

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
    type Item = windows::core::Result<SoundCoreFeature>;

    fn next(&mut self) -> Option<windows::core::Result<SoundCoreFeature>> {
        unsafe {
            let span = trace_span!(
                "Fetching feature",
                context = self.context,
                index = self.index,
            );
            let _span = span.enter();
            let mut info = MaybeUninit::uninit();
            let info = match self
                .target
                .EnumFeatures(self.context, self.index, info.as_mut_ptr())
                .ok()
            {
                Ok(()) => info.assume_init(),
                // FAIL used to mark end of collection
                Err(error) if error.code() == E_FAIL => return None,
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
