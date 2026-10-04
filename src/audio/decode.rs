//! Opening a music file and turning it into stereo samples.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{Channels, Position};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, Timestamp};

/// A file being decoded, packet by packet.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
    /// The samples of the last packet as the file has them, interleaved.
    packet: Vec<f32>,
    /// How much of each of the file's channels goes to the left and the right, made again only
    /// when the channels change.
    mix: (Channels, Vec<(f32, f32)>),
    /// After a seek, the moment asked for: packets that end before it are passed over, since a
    /// file can only be entered at the packet before.
    from: Option<Timestamp>,
}

impl Decoder {
    /// Opens the file at `path` and its first audio track.
    ///
    /// # Errors
    ///
    /// Says why when the file cannot be read, is not audio qmus can decode, or has no rate.
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        Self::open_source(Box::new(file), path.extension().and_then(|extension| extension.to_str()))
    }

    /// Opens `source`, a file or a track still arriving over the network, and its first audio
    /// track. `ending` is the file ending the source would have, when known: it helps tell formats
    /// apart.
    ///
    /// # Errors
    ///
    /// As [`Decoder::open`].
    pub fn open_source(source: Box<dyn MediaSource>, ending: Option<&str>) -> Result<Self, String> {
        let source = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(ending) = ending {
            hint.with_extension(ending);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, source, FormatOptions::default(), MetadataOptions::default())
            .map_err(|error| error.to_string())?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| "no audio track".to_owned())?;
        let params = track.codec_params.as_ref().and_then(|params| params.audio()).ok_or("no audio track")?;
        let rate = params.sample_rate.ok_or("no sample rate")?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|error| error.to_string())?;
        let track = track.id;
        Ok(Self { format, decoder, track, rate, packet: Vec::new(), mix: (Channels::None, Vec::new()), from: None })
    }

    /// Samples per second of each channel.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Moves to `at` from the start of the track, so the next samples decoded are the ones heard
    /// there, to within a packet.
    ///
    /// # Errors
    ///
    /// Says why when the file cannot be entered there, as past its end.
    pub fn seek(&mut self, at: std::time::Duration) -> Result<(), String> {
        let seconds = i64::try_from(at.as_secs()).map_err(|error| error.to_string())?;
        let time = Time::try_new(seconds, at.subsec_nanos()).ok_or("no such time")?;
        let landed = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track) })
            .map_err(|error| error.to_string())?;
        self.decoder.reset();
        self.from = Some(landed.required_ts);
        Ok(())
    }

    /// Decodes the next packet and appends it to `out` as interleaved stereo. Returns `false` at the
    /// end of the file. A packet that does not decode is skipped, as a player skips a scratch.
    ///
    /// # Errors
    ///
    /// Says why when the file can no longer be read.
    pub fn next_into(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(error) => return Err(error.to_string()),
            };
            if packet.track_id != self.track {
                continue;
            }
            if let Some(from) = self.from {
                if packet.pts.saturating_add(packet.dur) <= from {
                    continue;
                }
                self.from = None;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(Error::DecodeError(_)) => continue,
                Err(error) => return Err(error.to_string()),
            };
            let channels = decoded.spec().channels();
            if self.mix.0 != *channels {
                self.mix = (channels.clone(), mix(channels));
            }
            self.packet.resize(decoded.samples_interleaved(), 0.0);
            decoded.copy_to_slice_interleaved(&mut self.packet);
            stereo(&self.packet, &self.mix.1, out);
            return Ok(true);
        }
    }
}

/// How much of each channel of `channels` goes to the left and to the right.
///
/// Mono goes to both sides whole and stereo stays as it is. More channels are folded down the
/// usual way: the front left and right whole to their side, every other left or right channel at
/// −3 dB to its side, centre channels at −3 dB to both, the low-frequency channel left out; then
/// everything is scaled so that no side can go past full scale. Channels without a known place
/// keep the first two, as the front pair of every usual layout.
fn mix(channels: &Channels) -> Vec<(f32, f32)> {
    const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let count = channels.count().max(1);
    let gains: Vec<(f32, f32)> = match (count, channels) {
        (1, _) => vec![(1.0, 1.0)],
        (2, _) => vec![(1.0, 0.0), (0.0, 1.0)],
        (_, Channels::Positioned(positions)) => positions
            .iter()
            .map(|position| {
                let name = position.iter_names().next().map_or("", |(name, _)| name);
                if position == Position::FRONT_LEFT {
                    (1.0, 0.0)
                } else if position == Position::FRONT_RIGHT {
                    (0.0, 1.0)
                } else if name.starts_with("LFE") {
                    (0.0, 0.0)
                } else if name.contains("LEFT") {
                    (HALF_POWER, 0.0)
                } else if name.contains("RIGHT") {
                    (0.0, HALF_POWER)
                } else {
                    (HALF_POWER, HALF_POWER)
                }
            })
            .collect(),
        _ => (0..count).map(|index| [(1.0, 0.0), (0.0, 1.0)].get(index).copied().unwrap_or_default()).collect(),
    };
    let loudest = gains.iter().fold((0.0_f32, 0.0_f32), |(left, right), gain| (left + gain.0, right + gain.1));
    let scale = loudest.0.max(loudest.1).max(1.0);
    gains.into_iter().map(|(left, right)| (left / scale, right / scale)).collect()
}

/// Appends `samples`, interleaved over as many channels as `mix` has, to `out` as two channels.
fn stereo(samples: &[f32], mix: &[(f32, f32)], out: &mut Vec<f32>) {
    match mix.len() {
        0 => {}
        1 => out.extend(samples.iter().flat_map(|&sample| [sample, sample])),
        2 => out.extend_from_slice(samples),
        channels => out.extend(samples.chunks_exact(channels).flat_map(|frame| {
            frame.iter().zip(mix).fold([0.0, 0.0], |[left, right], (sample, (to_left, to_right))| {
                [left + sample * to_left, right + sample * to_right]
            })
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Scratch, sine_wav, sine_wav_on};

    #[test]
    fn a_wav_decodes_into_as_many_stereo_frames_as_it_holds() {
        let scratch = Scratch::new("decode");
        let path = scratch.path("tone.wav");
        sine_wav(&path, 22_050, 1, 0.2, 440.0);
        let mut decoder = Decoder::open(&path).expect("the file opens");
        assert_eq!(decoder.rate(), 22_050);
        let mut out = Vec::new();
        while decoder.next_into(&mut out).expect("the file reads") {}
        assert_eq!(out.len(), 4_410 * 2, "a fifth of a second of mono, doubled into two channels");
        assert!(out.iter().any(|sample| sample.abs() > 0.3), "the tone is there");
    }

    #[test]
    fn a_file_that_is_not_audio_says_so() {
        let scratch = Scratch::new("not-audio");
        let path = scratch.path("notes.mp3");
        std::fs::write(&path, "not music at all").expect("file");
        assert!(Decoder::open(&path).is_err());
    }

    #[test]
    fn the_centre_of_a_five_one_file_is_heard_on_both_sides() {
        let scratch = Scratch::new("decode-surround");
        let path = scratch.path("surround.wav");
        // Six channels in the usual order: front left, front right, centre, low-frequency,
        // rear left, rear right. Only the centre carries the tone, as a sung line often does.
        sine_wav_on(&path, 8_000, 6, 0.1, 440.0, 2);
        let mut decoder = Decoder::open(&path).expect("the file opens");
        let mut out = Vec::new();
        while decoder.next_into(&mut out).expect("the file reads") {}
        assert_eq!(out.len(), 800 * 2, "every frame, folded into two channels");
        let (left, right) = out
            .chunks_exact(2)
            .fold((0.0_f32, 0.0_f32), |(l, r), frame| (l.max(frame[0].abs()), r.max(frame[1].abs())));
        assert!(left > 0.1 && (left - right).abs() < 1e-6, "the centre is heard on both sides: {left} {right}");
    }

    #[test]
    fn a_five_one_frame_folds_each_channel_to_its_side_and_leaves_the_low_frequencies_out() {
        let channels = Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::REAR_LEFT
                | Position::REAR_RIGHT,
        );
        let gains = mix(&channels);
        let fold = |frame: [f32; 6]| {
            let mut out = Vec::new();
            stereo(&frame, &gains, &mut out);
            (out[0], out[1])
        };
        let (left, right) = fold([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(left > 0.0 && right == 0.0, "front left stays left: {left} {right}");
        let (left, right) = fold([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(left == 0.0 && right > 0.0, "rear right goes right: {left} {right}");
        assert_eq!(fold([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), (0.0, 0.0), "the low-frequency channel is left out");
        let (left, right) = fold([1.0; 6]);
        assert!(left <= 1.0 + 1e-6 && right <= 1.0 + 1e-6, "full scale everywhere does not clip: {left} {right}");
        let (left, right) = fold([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(fold([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), (right, left), "both sides alike");
    }
}
