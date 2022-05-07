use std::mem;

use tracing::trace_span;
use winapi::shared::winerror::E_FAIL;

use crate::com::ComObject;
use crate::ctsndcr::{ISoundCore, ParamInfo};
use crate::hresult::{check, Win32Error};

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
    type Item = Result<SoundCoreParameter, Win32Error>;

    fn next(&mut self) -> Option<Result<SoundCoreParameter, Win32Error>> {
        unsafe {
            let mut info: ParamInfo = mem::zeroed();
            let span = trace_span!(
                "Fetching parameter .{context}.{feature}[{index}]...",
                context = self.context,
                feature = %self.feature_description,
                index = self.index,
            );
            let _span = span.enter();
            match check(self.target.EnumParams(
                self.context,
                self.index,
                self.feature_id,
                &mut info,
            )) {
                Ok(_) => {}
                // FAIL used to mark end of collection
                Err(Win32Error { code, .. }) if code == E_FAIL => return None,
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
