use std::mem::MaybeUninit;

use tracing::trace_span;
use windows::Win32::Foundation::E_FAIL;

use crate::com::ComObject;
use crate::ctsndcr::ISoundCore;

use crate::SoundCoreParameter;

/// Iterates over the parameters of a feature.
pub struct SoundCoreParameterIterator {
    target: ComObject<ISoundCore>,
    context: u32,
    feature_id: u32,
    feature_description: String,
    index: u32,
}

impl SoundCoreParameterIterator {
    pub(crate) fn new(
        target: ComObject<ISoundCore>,
        context: u32,
        feature_id: u32,
        feature_description: String,
    ) -> Self {
        Self {
            target,
            context,
            feature_id,
            feature_description,
            index: 0,
        }
    }
}

impl Iterator for SoundCoreParameterIterator {
    type Item = windows::core::Result<SoundCoreParameter>;

    fn next(&mut self) -> Option<windows::core::Result<SoundCoreParameter>> {
        unsafe {
            let span = trace_span!(
                "Fetching parameter .{context}.{feature}[{index}]...",
                context = self.context,
                feature = %self.feature_description,
                index = self.index,
            );
            let _span = span.enter();
            let mut info = MaybeUninit::uninit();
            let info = match self
                .target
                .EnumParams(self.context, self.index, self.feature_id, info.as_mut_ptr())
                .ok()
            {
                Ok(()) => info.assume_init(),
                // FAIL used to mark end of collection
                Err(error) if error.code() == E_FAIL => return None,
                Err(error) => return Some(Err(error)),
            };
            span.record("info", &tracing::field::debug(&info));
            self.index += 1;
            match info.param.feature {
                0 => None,
                _ => Some(Ok(SoundCoreParameter::new(
                    self.target.clone(),
                    self.feature_description.clone(),
                    &info,
                ))),
            }
        }
    }
}
