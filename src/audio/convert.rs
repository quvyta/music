//! Changing a track's rate to the rate of the sound already going out, so a track of another rate
//! can follow in the same output with no gap.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

/// Frames taken in at a time: small enough to keep the delay short, large enough to be cheap.
const CHUNK: usize = 1024;

/// A rate change for interleaved stereo, fed as the track is decoded.
pub(crate) struct Converter {
    resampler: Fft<f32>,
    /// Samples taken in but not yet a whole chunk.
    pending: Vec<f32>,
    /// Where each chunk is written.
    out: Vec<f32>,
    /// Frames still to be dropped from the start: the resampler's own delay.
    delay: usize,
    /// Frames taken in since the start or the last seek.
    taken: u64,
    /// Frames given out since the start or the last seek.
    given: u64,
    /// The two rates, in and out.
    rates: (u32, u32),
}

impl Converter {
    /// A converter from `from` frames a second to `to`; `None` when the two cannot be converted.
    pub(crate) fn new(from: u32, to: u32) -> Option<Self> {
        let resampler = Fft::<f32>::new(from as usize, to as usize, CHUNK, 2, FixedSync::Input).ok()?;
        let out = vec![0.0; resampler.output_frames_max() * 2];
        let delay = resampler.output_delay();
        Some(Self { resampler, pending: Vec::new(), out, delay, taken: 0, given: 0, rates: (from, to) })
    }

    /// Takes in `samples` and adds what comes out of them to `into`.
    pub(crate) fn push(&mut self, samples: &[f32], into: &mut Vec<f32>) {
        self.pending.extend_from_slice(samples);
        self.taken += samples.len() as u64 / 2;
        loop {
            let frames = self.resampler.input_frames_next();
            if self.pending.len() / 2 < frames {
                return;
            }
            let (from, count) = self.run(frames, None);
            into.extend_from_slice(&self.out[from * 2..(from + count) * 2]);
            self.given += count as u64;
            self.pending.drain(..frames * 2);
        }
    }

    /// Lets out the rest once the track has been read to its end, so the converted track is as
    /// long as the track was: no frame lost to the resampler's delay, and none of its silence
    /// added.
    pub(crate) fn finish(&mut self, into: &mut Vec<f32>) {
        // A few rounds of silence are enough to flush the delay; the bound keeps a resampler that
        // would never say so from turning this into a loop without end.
        for _ in 0..8 {
            let owed = (self.taken * u64::from(self.rates.1) / u64::from(self.rates.0)).saturating_sub(self.given);
            if owed == 0 {
                break;
            }
            let real = self.pending.len() / 2;
            let frames = self.resampler.input_frames_next();
            self.pending.resize(frames.max(real) * 2, 0.0);
            let (from, count) = self.run(frames, Some(real.min(frames)));
            let count = count.min(usize::try_from(owed).unwrap_or(usize::MAX));
            into.extend_from_slice(&self.out[from * 2..(from + count) * 2]);
            self.given += count as u64;
            self.pending.drain(..(real.min(frames) * 2));
            self.pending.truncate(real.saturating_sub(frames) * 2);
        }
        self.pending.clear();
    }

    /// Starts again from nothing, as after a seek.
    pub(crate) fn reset(&mut self) {
        self.resampler.reset();
        self.pending.clear();
        self.delay = self.resampler.output_delay();
        self.taken = 0;
        self.given = 0;
    }

    /// Converts the first `frames` of what is pending, only `partial` of them real when given, and
    /// gives where in `out` the frames to keep begin and how many there are.
    fn run(&mut self, frames: usize, partial: Option<usize>) -> (usize, usize) {
        let capacity = self.out.len() / 2;
        let (Ok(input), Ok(mut output)) = (
            InterleavedSlice::new(&self.pending[..], 2, frames),
            InterleavedSlice::new_mut(&mut self.out[..], 2, capacity),
        ) else {
            return (0, 0);
        };
        let indexing = partial.map(|real| Indexing::new().partial_len(real));
        let Ok((_, written)) = self.resampler.process_into_buffer(&input, &mut output, indexing.as_ref()) else {
            return (0, 0);
        };
        let skip = self.delay.min(written);
        self.delay -= skip;
        (skip, written - skip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of a 1 kHz tone at `rate`, the same on both sides.
    fn tone(rate: u32) -> Vec<f32> {
        (0..rate)
            .flat_map(|at| {
                let sample = (std::f32::consts::TAU * 1000.0 * at as f32 / rate as f32).sin() * 0.5;
                [sample, sample]
            })
            .collect()
    }

    #[test]
    fn a_track_converted_lasts_as_long_as_it_did_and_keeps_its_tone() {
        let mut converter = Converter::new(44_100, 48_000).expect("a converter");
        let source = tone(44_100);
        let mut out = Vec::new();
        // Fed the way a decoder feeds it: packets of uneven length.
        for packet in source.chunks(1152 * 2 + 6) {
            converter.push(packet, &mut out);
        }
        converter.finish(&mut out);
        assert_eq!(out.len(), 48_000 * 2, "one second at the new rate, frame for frame");
        // The same tone at the new rate, past the edges, where the conversion has settled.
        let expected = tone(48_000);
        let worst = out[2000..94_000]
            .iter()
            .zip(&expected[2000..94_000])
            .map(|(got, want)| (got - want).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 0.01, "the tone survives the conversion: {worst}");
    }

    #[test]
    fn after_a_reset_it_starts_again_with_no_trace_of_before() {
        let mut converter = Converter::new(48_000, 44_100).expect("a converter");
        let mut out = Vec::new();
        converter.push(&tone(48_000)[..20_000], &mut out);
        converter.reset();
        let mut again = Vec::new();
        let source = tone(48_000);
        for packet in source.chunks(4096) {
            converter.push(packet, &mut again);
        }
        converter.finish(&mut again);
        assert_eq!(again.len(), 44_100 * 2);
    }
}
