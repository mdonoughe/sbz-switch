use std::str;

use slog::Logger;

use crate::com::ComObject;
use crate::ctsndcr::{FeatureInfo, ISoundCore};

use super::SoundCoreParameterIterator;

/// Represents a feature of a device.
#[derive(Debug)]
pub struct SoundCoreFeature {
    core: ComObject<ISoundCore>,
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
        core: ComObject<ISoundCore>,
        logger: Logger,
        context: u32,
        info: &FeatureInfo,
    ) -> Self {
        let description_length = info
            .description
            .iter()
            .position(|i| *i == 0)
            .unwrap_or(info.description.len());
        let version_length = info
            .version
            .iter()
            .position(|i| *i == 0)
            .unwrap_or(info.version.len());
        Self {
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
        }
    }
    /// Gets an iterator over the parameters of this feature.
    pub fn parameters(&self) -> SoundCoreParameterIterator {
        SoundCoreParameterIterator::new(
            self.core.clone(),
            self.logger.clone(),
            self.context,
            self.id,
            self.description.clone(),
        )
    }
}
