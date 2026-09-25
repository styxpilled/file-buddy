//! Plays sound faster or slower at the same pitch, with WSOLA: overlapping
//! windows of the input, each placed where it best continues the one before.
//! It works on interleaved stereo, the format of the audio ring.

use std::collections::VecDeque;

/// Frames of output for each window, and half the window.
const HOP_SECONDS: f64 = 0.010;

pub(crate) struct Stretcher {
    hop: usize,
    /// How far from its nominal place a window may go to match the last one.
    tolerance: usize,
    /// The rising and falling halves of a Hann window. Each pair sums to 1.
    rise: Vec<f32>,
    fall: Vec<f32>,
    /// 1x sound taken from the ring, interleaved. Frame 0 is the oldest kept.
    input: Vec<f32>,
    /// Where the next window belongs, in frames into `input`.
    pos: f64,
    /// Where the last window would go on: its start plus a hop, in frames
    /// into `input`. The next window should sound like the frames here.
    natural: Option<usize>,
    /// The falling half of the last window, to add to the next one.
    tail: Vec<f32>,
    out: VecDeque<f32>,
    /// Media samples played, less the whole ones already reported.
    media: f64,
}

impl Stretcher {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let hop = ((sample_rate as f64 * HOP_SECONDS) as usize).max(16);
        let window = hop * 2;
        let hann =
            |i: usize| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / window as f64).cos();
        Self {
            hop,
            tolerance: hop / 2,
            rise: (0..hop).map(|i| hann(i) as f32).collect(),
            fall: (hop..window).map(|i| hann(i) as f32).collect(),
            input: Vec::new(),
            pos: 0.0,
            natural: None,
            tail: vec![0.0; hop * 2],
            out: VecDeque::new(),
            media: 0.0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.input.clear();
        self.pos = 0.0;
        self.natural = None;
        self.tail.fill(0.0);
        self.out.clear();
        self.media = 0.0;
    }

    /// Fills `output` at `speed` with sound pulled by `pull`, which fills its
    /// slice from the front and returns how many samples it wrote. Returns the
    /// media samples that `output` played, for the clock. Silence fills what
    /// the input cannot.
    pub(crate) fn process(
        &mut self,
        output: &mut [f32],
        speed: f64,
        mut pull: impl FnMut(&mut [f32]) -> usize,
    ) -> u64 {
        while self.out.len() < output.len() {
            if !self.fill_input(&mut pull) {
                break;
            }
            self.step(speed);
            self.media += (self.hop * 2) as f64 * speed;
        }
        let ready = self.out.len().min(output.len());
        for (sample, value) in output.iter_mut().zip(self.out.drain(..ready)) {
            *sample = value;
        }
        output[ready..].fill(0.0);
        let whole = self.media.floor();
        self.media -= whole;
        whole as u64
    }

    pub(crate) fn idle(&self) -> bool {
        self.input.is_empty() && self.out.is_empty()
    }

    /// The 1x sound taken from the ring that has not played yet. The stretcher
    /// is empty after this.
    pub(crate) fn unplayed(&mut self) -> Vec<f32> {
        let from = (self.pos.round() as usize * 2).min(self.input.len());
        let rest = self.input.split_off(from);
        self.reset();
        rest
    }

    /// Makes sure the next window and its search range are in `input`.
    fn fill_input(&mut self, pull: &mut impl FnMut(&mut [f32]) -> usize) -> bool {
        let needed = (self.pos as usize + self.tolerance + self.hop * 2) * 2;
        if self.input.len() < needed {
            let have = self.input.len();
            self.input.resize(needed, 0.0);
            let got = pull(&mut self.input[have..]);
            self.input.truncate(have + got);
        }
        self.input.len() >= needed
    }

    fn step(&mut self, speed: f64) {
        let hop = self.hop;
        let nominal = self.pos.round() as usize;
        let start = match self.natural {
            None => nominal,
            Some(natural) => self.best_start(natural, nominal),
        };
        for i in 0..hop {
            for channel in 0..2 {
                let at = i * 2 + channel;
                self.out.push_back(
                    self.tail[at] + self.input[(start + i) * 2 + channel] * self.rise[i],
                );
                self.tail[at] = self.input[(start + hop + i) * 2 + channel] * self.fall[i];
            }
        }
        self.natural = Some(start + hop);
        self.pos += hop as f64 * speed;
        self.drop_played();
    }

    /// The start near `nominal` whose first half sounds most like the frames at
    /// `natural`, which is where the last window would have gone on.
    fn best_start(&self, natural: usize, nominal: usize) -> usize {
        let low = nominal.saturating_sub(self.tolerance);
        let high = nominal + self.tolerance;
        let template = &self.input[natural * 2..(natural + self.hop) * 2];
        let mut best = (f32::MIN, nominal);
        // Every second frame is enough to find the match, at half the cost.
        for start in low..=high {
            let candidate = &self.input[start * 2..(start + self.hop) * 2];
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for (a, b) in template.iter().zip(candidate).step_by(4) {
                dot += a * b;
                energy += b * b;
            }
            let score = dot / energy.sqrt();
            if score > best.0 {
                best = (score, start);
            }
        }
        best.1
    }

    /// Forgets input that no later window or search can reach.
    fn drop_played(&mut self) {
        let Some(natural) = self.natural else {
            return;
        };
        let keep = natural.min((self.pos as usize).saturating_sub(self.tolerance));
        if keep == 0 {
            return;
        }
        self.input.drain(..keep * 2);
        self.pos -= keep as f64;
        self.natural = Some(natural - keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(frequency: f64, seconds: f64) -> Vec<f32> {
        let frames = (RATE as f64 * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let value = (2.0 * std::f64::consts::PI * frequency * i as f64 / RATE as f64).sin();
                [value as f32, value as f32]
            })
            .collect()
    }

    /// Plays all of `input` at `speed` in blocks, as the device asks for it.
    fn play(input: &[f32], speed: f64) -> (Vec<f32>, u64) {
        let mut stretcher = Stretcher::new(RATE);
        let mut read = 0;
        let mut output = Vec::new();
        let mut media = 0;
        loop {
            let mut block = vec![0.0; 960];
            media += stretcher.process(&mut block, speed, |slice| {
                let n = slice.len().min(input.len() - read);
                slice[..n].copy_from_slice(&input[read..read + n]);
                read += n;
                n
            });
            let silent = block.iter().all(|s| *s == 0.0);
            output.extend(block);
            if silent && read == input.len() {
                return (output, media);
            }
        }
    }

    fn crossings(samples: &[f32]) -> usize {
        let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
        left.windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn twice_the_speed_halves_the_length_and_keeps_the_pitch() {
        let input = sine(440.0, 2.0);
        let (output, media) = play(&input, 2.0);
        let sound: Vec<f32> = output
            .iter()
            .copied()
            .rev()
            .skip_while(|s| *s == 0.0)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let seconds = sound.len() as f64 / 2.0 / RATE as f64;
        assert!((seconds - 1.0).abs() < 0.05, "{seconds} s");
        let pitch = crossings(&sound) as f64 / seconds;
        assert!((pitch - 440.0).abs() < 10.0, "{pitch} Hz");
        // The clock moved through the whole input, less the search margin at
        // the end.
        let played = media as f64 / 2.0 / RATE as f64;
        assert!((played - 2.0).abs() < 0.05, "{played} s");
    }

    #[test]
    fn a_sine_comes_out_without_clicks() {
        let (output, _) = play(&sine(440.0, 1.0), 2.0);
        let left: Vec<f32> = output.iter().step_by(2).copied().collect();
        let end = left.iter().rposition(|s| *s != 0.0).unwrap();
        // The first window fades in from silence.
        let steady = &left[RATE as usize / 100..end - RATE as usize / 100];
        // A 440 Hz sine moves at most 0.058 from one sample to the next. A
        // window placed out of phase would jump.
        let jump = steady
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(jump < 0.08, "{jump}");
    }

    #[test]
    fn the_unplayed_sound_is_handed_back() {
        let input = sine(440.0, 0.5);
        let mut stretcher = Stretcher::new(RATE);
        let mut block = vec![0.0; 960];
        let mut read = 0;
        stretcher.process(&mut block, 2.0, |slice| {
            let n = slice.len().min(input.len() - read);
            slice[..n].copy_from_slice(&input[read..read + n]);
            read += n;
            n
        });
        let rest = stretcher.unplayed();
        assert!(!rest.is_empty());
        assert!(rest.len() < read);
        assert_eq!(rest.len() % 2, 0);
    }
}
