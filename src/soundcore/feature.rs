use std::ptr::NonNull;
use std::str;

use slog::Logger;

use ctsndcr::{FeatureInfo, ISoundCore};

use super::SoundCoreParameterIterator;

/// Represents a feature of a device.
#[derive(Debug)]
pub struct SoundCoreFeature {
    core: NonNull<ISoundCore>,
    logger: Logger,
    context: u32,
    /// A numeric ID of the feature
    pub id: u32,
    /// A description of the feature
    pub description: String,
    /// A version number of the feature implementation
    pub version: String,
}

impl SoundCoreFeature {
    pub(crate) fn new(
        mut core: NonNull<ISoundCore>,
        logger: Logger,
        context: u32,
        info: &FeatureInfo,
    ) -> Self {
        let description_length = info
            .description
            .iter()
            .position(|i| *i == 0)
            .unwrap_or_else(|| info.description.len());
        let version_length = info
            .version
            .iter()
            .position(|i| *i == 0)
            .unwrap_or_else(|| info.version.len());
        let result = Self {
            core,
            logger,
            context,
            id: info.feature_id,
            description: str::from_utf8(&info.description[0..description_length])
                .unwrap()
                .to_owned(),
            version: str::from_utf8(&info.version[0..version_length])
                .unwrap()
                .to_owned(),
        };
        unsafe {
            core.as_mut().AddRef();
        }
        result
    }
    /// Gets an iterator over the parameters of this feature.
    pub fn parameters(&self) -> SoundCoreParameterIterator {
        SoundCoreParameterIterator::new(
            self.core,
            self.logger.clone(),
            self.context,
            self.id,
            self.description.clone(),
        )
    }
}

impl Drop for SoundCoreFeature {
    fn drop(&mut self) {
        unsafe {
            self.core.as_mut().Release();
        }
    }
}
