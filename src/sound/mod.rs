use crate::Result;

use std::io::Cursor;

use rodio::MixerDeviceSink;
use rodio::mixer::Mixer;
use rodio::speakers::SpeakersBuilder;

pub struct Sound {
    _stream: MixerDeviceSink,
    mixer: Mixer,
    cursor: Cursor<&'static [u8; 5943]>,
}

impl Sound {
    pub fn new() -> Result<Self> {
        // The speakers builder validates the config against the device's
        // supported ranges and clamps the buffer size accordingly (the legacy
        // DeviceSinkBuilder requests a fixed ~50ms buffer, e.g. 2048 frames,
        // which some devices reject).
        let mut stream = SpeakersBuilder::new()
            .default_device()?
            .default_config()?
            .open_mixer()?;
        // `play()` waits for the sound to finish before dropping, so the
        // default drop notice is just noise.
        stream.log_on_drop(false);
        let mixer = stream.mixer().clone();
        let contents = include_bytes!("beep.ogg");
        Ok(Sound {
            _stream: stream,
            mixer,
            cursor: Cursor::new(contents),
        })
    }

    /// Play sound (it has a delay of ~100ms)
    pub fn play(&self) -> Result<()> {
        let cursor = self.cursor.clone();
        let decoder = rodio::Decoder::new(cursor)?;
        let player = rodio::Player::connect_new(&self.mixer);
        player.append(decoder);
        player.sleep_until_end();
        Ok(())
    }
}
