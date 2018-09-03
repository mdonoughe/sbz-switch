//! Provides a Rust API over Creative's COM SoundCore API.
//!
//! 1. Use [`media`](../media) to find a device.
//! 2. Use `get_sound_core` to get a `SoundCore` for that device.
//!
//! # COM Warning
//!
//! Usage of this API requires that CoInitialize has been called on the current thread.

mod consts;
mod core;
mod error;
mod event;
mod feature;
mod feature_iterator;
mod parameter;
mod parameter_iterator;

pub use self::consts::PKEY_SOUNDCORECTL_CLSID;
pub use self::core::{get_sound_core, SoundCore};
pub use self::error::SoundCoreError;
pub use self::event::{SoundCoreEvent, SoundCoreEventIterator};
pub use self::feature::SoundCoreFeature;
pub use self::feature_iterator::SoundCoreFeatureIterator;
pub use self::parameter::{SoundCoreParamValue, SoundCoreParameter};
pub use self::parameter_iterator::SoundCoreParameterIterator;
