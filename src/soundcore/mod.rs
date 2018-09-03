//! Provides a Rust layer over Creative's COM SoundCore API.
//!
//! 1. Use [`media::DeviceEnumerator`](../media/struct.DeviceEnumerator.html)
//!    to find a device.
//! 2. Use [`SoundCore::for_device`](struct.SoundCore.html#method.for_device)
//!    to get a `SoundCore` for that device.

mod consts;
mod core;
mod error;
mod event;
mod feature;
mod feature_iterator;
mod parameter;
mod parameter_iterator;

pub use self::consts::PKEY_SOUNDCORECTL_CLSID;
pub use self::core::SoundCore;
pub use self::error::SoundCoreError;
pub use self::event::{SoundCoreEvent, SoundCoreEventIterator};
pub use self::feature::SoundCoreFeature;
pub use self::feature_iterator::SoundCoreFeatureIterator;
pub use self::parameter::{SoundCoreParamValue, SoundCoreParameter};
pub use self::parameter_iterator::SoundCoreParameterIterator;
