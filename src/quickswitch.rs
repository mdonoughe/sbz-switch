
#[derive(Serialize, Deserialize)]
pub struct QuickSwitchConfig {
    pub headphone_dev_id: u32,
    pub speaker_dev_id: u32,
    pub headphone_vol: f32,
    pub speaker_vol: f32,
    pub mute: bool,
}

// Default Values for Quick Switch config
impl ::std::default::Default for QuickSwitchConfig {
    fn default() -> Self {
        Self {
            headphone_dev_id: 0,
            speaker_dev_id: 1,
            headphone_vol: 8.0,
            speaker_vol: 100.0,
            mute: true,
        }
    }
}