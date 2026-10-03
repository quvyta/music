//! The visualizer's measure of the sound: how loud each band of frequencies is in the samples that
//! went out, moving the way a meter's needle moves, fast up and slowly down.

use std::sync::Arc;
use std::time::Duration;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Samples in one measure: 2048 is 43 ms of sound at 48 kHz, with 23 Hz between two frequencies
/// told apart.
pub const WINDOW: usize = 2048;

/// The lowest frequency shown, in hertz; below it is felt more than heard.
const LOWEST: f32 = 30.0;

/// The highest frequency shown, in hertz, unless the sound's rate cannot carry it.
const HIGHEST: f32 = 16_000.0;

/// The quietest level shown, in decibels below a full-scale tone; anything quieter is silence.
const FLOOR_DB: f32 = -60.0;

/// How quickly a band rises toward a louder sound: the time it takes to cover most of the way.
const RISE: f32 = 0.030;

/// How quickly a band falls toward a quieter sound.
const FALL: f32 = 0.250;

/// How fast a peak cap falls once the band below it has fallen, in levels a second.
const PEAK_FALL: f32 = 0.5;

/// Below this a band counts as resting.
const STILL: f32 = 0.002;

/// How loud each of a number of bands is, from the lowest frequency to the highest, each a level
/// between zero (silence) and one (a full-scale tone), with a cap over each that holds the band's
/// high mark and falls slowly after it.
pub struct Spectrum {
    levels: Vec<f32>,
    peaks: Vec<f32>,
    /// What the last measure found, before any smoothing.
    measured: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
    /// The Hann window, so a band does not bleed into its neighbours.
    window: Vec<f32>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl std::fmt::Debug for Spectrum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spectrum").field("levels", &self.levels).field("peaks", &self.peaks).finish_non_exhaustive()
    }
}

impl Spectrum {
    /// A spectrum of `bands` bands, all silent.
    #[must_use]
    pub fn new(bands: usize) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let window = (0..WINDOW)
            .map(|index| {
                let phase = std::f32::consts::TAU * index as f32 / (WINDOW - 1) as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        Self {
            levels: vec![0.0; bands],
            peaks: vec![0.0; bands],
            measured: vec![0.0; bands],
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
        }
    }

    /// Each band's level now.
    #[must_use]
    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    /// Each band's cap: its high mark, falling slowly.
    #[must_use]
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    /// Each band's level as the last measure found it, with no movement smoothed in.
    #[must_use]
    pub fn measured(&self) -> &[f32] {
        &self.measured
    }

    /// Whether every band and cap has come down to silence, so nothing more would move.
    #[must_use]
    pub fn resting(&self) -> bool {
        self.levels.iter().chain(&self.peaks).all(|level| *level <= STILL)
    }

    /// Measures `samples`, the last of the sound heard at `rate` frames a second (the newest last,
    /// [`WINDOW`] of them; fewer are taken as silence before them), and moves the bands toward
    /// it over `elapsed`.
    pub fn hear(&mut self, samples: &[f32], rate: u32, elapsed: Duration) {
        self.measure(samples, rate);
        self.approach(elapsed);
    }

    /// Moves the bands toward silence over `elapsed`, as when the sound is held.
    pub fn quiet(&mut self, elapsed: Duration) {
        self.measured.fill(0.0);
        self.approach(elapsed);
    }

    /// Finds how loud each band is in `samples`, into `measured`.
    fn measure(&mut self, samples: &[f32], rate: u32) {
        let taken = &samples[samples.len().saturating_sub(WINDOW)..];
        let silent = WINDOW - taken.len();
        self.input[..silent].fill(0.0);
        for ((slot, sample), weight) in self.input[silent..].iter_mut().zip(taken).zip(&self.window[silent..]) {
            *slot = sample * weight;
        }
        if rate == 0 || self.fft.process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch).is_err() {
            self.measured.fill(0.0);
            return;
        }
        let rate = rate as f32;
        // A full-scale tone through the Hann window comes out at a quarter of the window.
        let full = WINDOW as f32 / 4.0;
        let step = rate / WINDOW as f32;
        let highest = HIGHEST.min(rate * 0.45);
        let bands = self.measured.len();
        let last = self.output.len() - 1;
        for (band, measured) in self.measured.iter_mut().enumerate() {
            let edge = |at: usize| LOWEST * (highest / LOWEST).powf(at as f32 / bands as f32);
            let (low, high) = (edge(band) / step, edge(band + 1) / step);
            // A band narrower than the distance between two frequencies told apart takes the one
            // nearest its middle.
            let first = (low.round() as usize).min(last);
            let through = (high.round() as usize).clamp(first, last);
            let loudest = self.output[first..=through].iter().map(|bin| bin.norm()).fold(0.0, f32::max);
            let decibels = 20.0 * (loudest / full).max(1e-9).log10();
            *measured = ((decibels - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
        }
    }

    /// Moves each band toward what was measured, quickly up and slowly down, and its cap with it.
    fn approach(&mut self, elapsed: Duration) {
        let seconds = elapsed.as_secs_f32();
        let rise = 1.0 - (-seconds / RISE).exp();
        let fall = 1.0 - (-seconds / FALL).exp();
        for ((level, peak), measured) in self.levels.iter_mut().zip(&mut self.peaks).zip(&self.measured) {
            let share = if *measured > *level { rise } else { fall };
            *level += (measured - *level) * share;
            *peak = if *level >= *peak { *level } else { (*peak - PEAK_FALL * seconds).max(*level) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of a tone of `hertz` at `rate`, `loud` of full scale.
    fn tone(hertz: f32, rate: u32, loud: f32) -> Vec<f32> {
        (0..WINDOW).map(|at| loud * (std::f32::consts::TAU * hertz * at as f32 / rate as f32).sin()).collect()
    }

    /// The band of `bands` a tone of `hertz` belongs in.
    fn band_of(hertz: f32, bands: usize, rate: u32) -> usize {
        let highest = HIGHEST.min(rate as f32 * 0.45);
        ((hertz / LOWEST).ln() / (highest / LOWEST).ln() * bands as f32) as usize
    }

    #[test]
    fn a_tone_lifts_its_own_band_and_leaves_the_far_ones_down() {
        for (hertz, rate) in [(1_000.0, 48_000), (120.0, 44_100), (6_000.0, 44_100)] {
            let mut spectrum = Spectrum::new(16);
            spectrum.hear(&tone(hertz, rate, 0.5), rate, Duration::from_secs(2));
            let own = band_of(hertz, 16, rate);
            let levels = spectrum.levels();
            let loudest = levels.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(band, _)| band);
            assert_eq!(loudest, Some(own), "{hertz} Hz at {rate}: {levels:?}");
            assert!(levels[own] > 0.8, "{hertz} Hz: {levels:?}");
            for (band, level) in levels.iter().enumerate() {
                if band.abs_diff(own) > 2 {
                    assert!(*level < 0.35, "{hertz} Hz, band {band}: {levels:?}");
                }
            }
        }
    }

    #[test]
    fn a_louder_tone_stands_higher_and_silence_shows_nothing() {
        let level = |loud: f32| {
            let mut spectrum = Spectrum::new(8);
            spectrum.hear(&tone(1_000.0, 48_000, loud), 48_000, Duration::from_secs(2));
            spectrum.levels().iter().copied().fold(0.0, f32::max)
        };
        assert!(level(0.5) > level(0.05) + 0.2, "{} against {}", level(0.5), level(0.05));
        let mut spectrum = Spectrum::new(8);
        spectrum.hear(&[0.0; WINDOW], 48_000, Duration::from_secs(2));
        assert!(spectrum.resting(), "{spectrum:?}");
    }

    #[test]
    fn a_band_rises_at_once_and_falls_slowly_under_its_falling_cap() {
        let mut spectrum = Spectrum::new(8);
        let sound = tone(1_000.0, 48_000, 0.5);
        let own = band_of(1_000.0, 8, 48_000);
        spectrum.hear(&sound, 48_000, Duration::from_millis(33));
        let target = spectrum.measured()[own];
        let risen = spectrum.levels()[own];
        assert!(risen > 0.6 * target, "most of the way up in a frame: {risen} of {target}");
        spectrum.hear(&sound, 48_000, Duration::from_millis(300));
        let top = spectrum.levels()[own];
        spectrum.quiet(Duration::from_millis(33));
        let fallen = spectrum.levels()[own];
        assert!(fallen > 0.8 * top, "only a little of the way down in a frame: {fallen} of {top}");
        assert!(spectrum.peaks()[own] > fallen, "the cap stays above the falling band");
        spectrum.quiet(Duration::from_millis(400));
        assert!(spectrum.peaks()[own] < top, "the cap falls too");
        for _ in 0..30 {
            spectrum.quiet(Duration::from_millis(100));
        }
        assert!(spectrum.resting(), "{spectrum:?}");
    }

    #[test]
    fn a_short_measure_counts_the_missing_samples_as_silence() {
        let mut spectrum = Spectrum::new(8);
        spectrum.hear(&tone(1_000.0, 48_000, 0.5)[..WINDOW / 2], 48_000, Duration::from_secs(2));
        assert!(spectrum.levels()[band_of(1_000.0, 8, 48_000)] > 0.6, "{spectrum:?}");
        let mut none = Spectrum::new(8);
        none.hear(&[], 48_000, Duration::from_secs(2));
        assert!(none.resting(), "{none:?}");
    }
}
