//! Sound-petting detection: the official `pet-detect` model, in the core.
//!
//! The physical duck hears a head scratch through its onboard mic and counts
//! it as a pet. The phone has a microphone too, and the same ~20 KB CNN —
//! `upstream/microduck/pet-detect/models/pet_detect.onnx` — decides the same
//! thing. This module is the feature path under that model, ported from
//! `pet-detect/src/lib.rs` with every number kept: 40 log-mel bands over a
//! 16 240-sample window at 16 kHz, 512-point FFT, 400-sample Hann window,
//! 160-sample hop, 100 frames, input `[1, 1, 40, 100]`, output class 1.
//!
//! It is self-contained on purpose: a pure-Rust radix-2 FFT (512 = 2^9) and
//! a small base64 decoder replace `rustfft`/`base64`, so the core gains no
//! new crates. The only dependency is the `ort` already linked for the
//! policies, at the same pinned version (`=2.0.0-rc.11`).
//!
//! The mel layout is the training contract: the official `pet-features`
//! binary exists so training and inference share it. Those constants moving
//! means retraining, so they are pinned in a unit test.
//!
//! **The duck's 61-D observation is untouched.** This is a sound event that
//! reaches the companion layer and the speaker channel, exactly like the
//! clap/whistle watch in [`crate::sensors`] — never a sense the policy sees.

use std::f32::consts::PI;
use std::path::Path;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

/// Capture rate the model was trained at.
pub const SAMPLE_RATE: usize = 16_000;
/// FFT size; 512 = 2^9, so the radix-2 transform needs no mixed-radix path.
pub const N_FFT: usize = 512;
/// 10 ms hop.
pub const HOP: usize = 160;
/// 25 ms analysis window.
pub const WIN: usize = 400;
pub const N_MELS: usize = 40;
/// 1.0 s of audio at the model's stride — 100 frames.
pub const WINDOW_FRAMES: usize = 100;
/// The model's analysis window: `(100 - 1) * 160 + 400 = 16_240` samples,
/// 1.015 s. The shell may post a 16 000-sample second; the detector keeps a
/// ring and runs once this many samples exist.
pub const WINDOW_SAMPLES: usize = (WINDOW_FRAMES - 1) * HOP + WIN;
pub const FMIN: f32 = 0.0;
pub const FMAX: f32 = 8_000.0;
pub const LOG_EPS: f32 = 1e-6;

/// Official `PettingDetectorConfig::default`: inference every quarter
/// window (≈ 250 ms), enter at 0.95, leave at 0.85 so the boundary does not
/// flap.
pub const DEFAULT_STRIDE: usize = WINDOW_SAMPLES / 4;
pub const DEFAULT_ENTER: f32 = 0.95;
pub const DEFAULT_EXIT: f32 = 0.85;

/// Start/End edges. A pet is counted on Start, never on the held state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PettingEvent {
    Start,
    End,
}

/// KNOWN pitfall: the ONNX Runtime session is not serialised by a checkpoint,
/// so the runtime reattaches this model by path after a fork or mode switch.
pub struct PettingDetector {
    session: Session,
    extractor: MelExtractor,
    ring: Vec<f32>,
    samples_until_infer: usize,
    stride: usize,
    is_petting: bool,
    enter_threshold: f32,
    exit_threshold: f32,
    /// How many times a window actually reached the model. Introspection for
    /// tests and a future `/health` counter; nothing else reads it.
    inferences: u64,
}

impl PettingDetector {
    /// Load the official model. Single-threaded on purpose, as upstream: for
    /// a 20 KB network a thread pool costs more than it recovers.
    ///
    /// `ort` panics on failures it considers unrecoverable (a version
    /// mismatch is the one upstream has seen); the panic is turned into an
    /// ordinary error so a missing or broken model disables the feature
    /// instead of taking the app down.
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let session = Session::builder()
                .and_then(|builder| builder.with_optimization_level(GraphOptimizationLevel::Level3))
                .and_then(|builder| builder.with_intra_threads(1))
                .and_then(|builder| builder.with_inter_threads(1))
                .and_then(|builder| builder.commit_from_file(model_path))
                .map_err(|error| format!("{}: {error}", model_path.display()))?;
            Ok(Self::with_session(session, DetectorConfig::default()))
        }));
        match outcome {
            Ok(result) => result,
            Err(payload) => {
                let detail = payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_owned());
                Err(format!(
                    "{}: ort panicked loading the pet model: {detail}",
                    model_path.display()
                ))
            }
        }
    }

    /// A detector with non-default hysteresis, for tests that need a window
    /// to count as a pet without a real scratch recording. Production always
    /// goes through [`Self::load`] and the official thresholds.
    pub fn load_with_config(model_path: &Path, config: DetectorConfig) -> Result<Self, String> {
        let session = Session::builder()
            .and_then(|builder| builder.with_optimization_level(GraphOptimizationLevel::Level3))
            .and_then(|builder| builder.with_intra_threads(1))
            .and_then(|builder| builder.with_inter_threads(1))
            .and_then(|builder| builder.commit_from_file(model_path))
            .map_err(|error| format!("{}: {error}", model_path.display()))?;
        Ok(Self::with_session(session, config))
    }

    fn with_session(session: Session, config: DetectorConfig) -> Self {
        Self {
            session,
            extractor: MelExtractor::new(),
            ring: Vec::with_capacity(WINDOW_SAMPLES * 2),
            samples_until_infer: WINDOW_SAMPLES,
            stride: config.stride.max(1),
            is_petting: false,
            enter_threshold: config.enter_threshold,
            exit_threshold: config.exit_threshold,
            inferences: 0,
        }
    }

    /// Push 16 kHz mono f32 samples in `[-1, 1]`, the official streaming
    /// contract: the first inference runs when the first full window exists,
    /// then every `stride` samples. Events are the transitions that happened
    /// in this push.
    ///
    /// `audible` is the room-sound gate: false skips the FFTs and the forward
    /// pass while *not* petting, but still advances the ring and schedule.
    /// This core passes `true` — the shell only posts PCM when it has decided
    /// the feature is on, and inventing a second RMS floor here would drift
    /// from the shell's.
    pub fn push_samples(
        &mut self,
        samples: &[f32],
        audible: bool,
    ) -> Result<Vec<PettingEvent>, String> {
        self.ring.extend_from_slice(samples);
        let mut events = Vec::new();

        while self.ring.len() >= self.samples_until_infer {
            let start = self.samples_until_infer - WINDOW_SAMPLES;
            // Advanced before the skip, so every path out of the body moves
            // the schedule — upstream's comment, and the reason a skip does
            // not spin on one window.
            self.samples_until_infer += self.stride;

            if !audible && !self.is_petting {
                continue;
            }

            let mel = self
                .extractor
                .log_mel(&self.ring[start..start + WINDOW_SAMPLES]);
            let input = Tensor::from_array(([1usize, 1, N_MELS, WINDOW_FRAMES], mel))
                .map_err(|error| format!("pet tensor: {error}"))?;
            // The session borrow ends with `outputs`; the hysteresis above
            // needs the detector mutably.
            let probability = {
                let outputs = self
                    .session
                    .run(ort::inputs![input])
                    .map_err(|error| format!("pet inference: {error}"))?;
                let (_shape, probs) = outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|error| format!("pet output: {error}"))?;
                // Two classes; index 1 is "petting". Official reads probs[1].
                probs.get(1).copied().ok_or_else(|| {
                    format!("pet model returned {} classes, expected 2", probs.len())
                })?
            };
            self.inferences += 1;
            if let Some(event) = self.hysteresis(probability) {
                events.push(event);
            }
        }

        // Cap the ring at 2× a window so memory is bounded, exactly upstream.
        if self.ring.len() > WINDOW_SAMPLES * 4 {
            let drop = self.ring.len() - WINDOW_SAMPLES * 2;
            self.ring.drain(..drop);
            self.samples_until_infer -= drop;
        }
        Ok(events)
    }

    /// Official hysteresis: enter at ≥ 0.95, leave below 0.85.
    fn hysteresis(&mut self, probability: f32) -> Option<PettingEvent> {
        if !self.is_petting && probability >= self.enter_threshold {
            self.is_petting = true;
            Some(PettingEvent::Start)
        } else if self.is_petting && probability < self.exit_threshold {
            self.is_petting = false;
            Some(PettingEvent::End)
        } else {
            None
        }
    }

    pub fn is_petting(&self) -> bool {
        self.is_petting
    }

    /// Put the verdict back after a rewind. The checkpoint carries the
    /// event state; the ring belonged to a future the duck no longer had and
    /// is dropped.
    pub fn set_petting(&mut self, petting: bool) {
        self.is_petting = petting;
    }

    /// Forget buffered audio and the verdict. A restore or a rewind is a new
    /// timeline: the samples that led to the old ring are not evidence about
    /// this one.
    pub fn reset(&mut self) {
        self.ring.clear();
        self.samples_until_infer = WINDOW_SAMPLES;
        self.is_petting = false;
    }

    /// Samples currently buffered. Tests assert the pause gate does not feed
    /// the detector; production does not read it.
    pub fn buffered_samples(&self) -> usize {
        self.ring.len()
    }

    /// Windows that reached the model. Introspection, as above.
    pub fn inferences(&self) -> u64 {
        self.inferences
    }
}

/// The official default: quarter-window stride, 0.95/0.85 hysteresis.
pub struct DetectorConfig {
    pub stride: usize,
    pub enter_threshold: f32,
    pub exit_threshold: f32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            stride: DEFAULT_STRIDE,
            enter_threshold: DEFAULT_ENTER,
            exit_threshold: DEFAULT_EXIT,
        }
    }
}

/// The feature path, unchanged from the official `MelExtractor`.
pub struct MelExtractor {
    /// Twiddles for the forward transform: `exp(-2πi k / N_FFT)` for
    /// `k < N_FFT / 2`.
    twiddles: Vec<Complex>,
    hann: Vec<f32>,
    /// Sparse: for each mel band, (fft_bin, weight).
    mel_filters: Vec<Vec<(usize, f32)>>,
    /// Reused FFT/工作 buffers so a window allocates nothing per frame.
    fft: Vec<Complex>,
    power: Vec<f32>,
}

impl MelExtractor {
    pub fn new() -> Self {
        let twiddles = (0..N_FFT / 2)
            .map(|k| {
                let angle = -2.0 * PI * k as f32 / N_FFT as f32;
                Complex::new(angle.cos(), angle.sin())
            })
            .collect();
        let hann: Vec<f32> = (0..WIN)
            .map(|n| 0.5 - 0.5 * (2.0 * PI * n as f32 / (WIN as f32 - 1.0)).cos())
            .collect();
        let mel_filters = build_mel_filterbank(SAMPLE_RATE, N_FFT, N_MELS, FMIN, FMAX);
        Self {
            twiddles,
            hann,
            mel_filters,
            fft: vec![Complex::new(0.0, 0.0); N_FFT],
            power: vec![0.0; N_FFT / 2 + 1],
        }
    }

    /// Process a window of mono f32 samples in `[-1, 1]`. Returns the
    /// log-mel matrix flattened row-major, `[N_MELS * WINDOW_FRAMES]`, band
    /// index major exactly as the official extractor writes it.
    pub fn log_mel(&mut self, samples: &[f32]) -> Vec<f32> {
        assert!(
            samples.len() >= WINDOW_SAMPLES,
            "need at least {WINDOW_SAMPLES} samples, got {}",
            samples.len()
        );
        let mut out = vec![0.0f32; N_MELS * WINDOW_FRAMES];

        for frame in 0..WINDOW_FRAMES {
            let start = frame * HOP;
            for (i, slot) in self.fft.iter_mut().enumerate() {
                *slot = if i < WIN {
                    Complex::new(samples[start + i] * self.hann[i], 0.0)
                } else {
                    Complex::new(0.0, 0.0)
                };
            }
            fft_forward(&mut self.fft, &self.twiddles);

            for (bin, slot) in self.power.iter_mut().enumerate() {
                let value = self.fft[bin];
                *slot = value.re * value.re + value.im * value.im;
            }

            for (m, filter) in self.mel_filters.iter().enumerate() {
                let mut sum = 0.0;
                for &(bin, weight) in filter {
                    sum += weight * self.power[bin];
                }
                out[m * WINDOW_FRAMES + frame] = (sum + LOG_EPS).ln();
            }
        }
        out
    }
}

impl Default for MelExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// One complex number, `f32` like the official path.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
}

/// In-place iterative radix-2 Cooley–Tukey forward FFT. `buffer.len()` must
/// be a power of two; `twiddles` is `exp(-2πi k / N)` for `k < N/2`.
fn fft_forward(buffer: &mut [Complex], twiddles: &[Complex]) {
    let n = buffer.len();
    debug_assert!(n.is_power_of_two());
    debug_assert_eq!(twiddles.len(), n / 2);

    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            buffer.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let step = n / len;
        let mut base = 0;
        while base < n {
            for k in 0..half {
                let w = twiddles[k * step];
                let u = buffer[base + k];
                let v = buffer[base + k + half];
                let t = Complex::new(v.re * w.re - v.im * w.im, v.re * w.im + v.im * w.re);
                buffer[base + k] = Complex::new(u.re + t.re, u.im + t.im);
                buffer[base + k + half] = Complex::new(u.re - t.re, u.im - t.im);
            }
            base += len;
        }
        len <<= 1;
    }
}

fn hz_to_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}

fn mel_to_hz(m: f32) -> f32 {
    700.0 * (10f32.powf(m / 2595.0) - 1.0)
}

/// The official triangular filterbank over `n_mels + 2` evenly spaced mel
/// points. Verbatim from `pet-detect/src/lib.rs`: this is the training
/// contract, not a tuning choice.
fn build_mel_filterbank(
    sr: usize,
    n_fft: usize,
    n_mels: usize,
    fmin: f32,
    fmax: f32,
) -> Vec<Vec<(usize, f32)>> {
    let n_bins = n_fft / 2 + 1;
    let mel_min = hz_to_mel(fmin);
    let mel_max = hz_to_mel(fmax);
    let hz_points: Vec<f32> = (0..n_mels + 2)
        .map(|i| mel_to_hz(mel_min + (mel_max - mel_min) * i as f32 / (n_mels + 1) as f32))
        .collect();
    let bin_hz: Vec<f32> = (0..n_bins)
        .map(|k| k as f32 * sr as f32 / n_fft as f32)
        .collect();

    let mut filters = Vec::with_capacity(n_mels);
    for m in 0..n_mels {
        let (lo, ctr, hi) = (hz_points[m], hz_points[m + 1], hz_points[m + 2]);
        let mut filter = Vec::new();
        for (k, &frequency) in bin_hz.iter().enumerate() {
            let weight = if frequency <= lo || frequency >= hi {
                0.0
            } else if frequency <= ctr {
                (frequency - lo) / (ctr - lo)
            } else {
                (hi - frequency) / (hi - ctr)
            };
            if weight > 0.0 {
                filter.push((k, weight));
            }
        }
        filters.push(filter);
    }
    filters
}

/// `i16` little-endian samples (what the shells' recorders produce) to f32
/// in `[-1, 1]`, the official conversion.
pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
}

/// Linear resampling to [`SAMPLE_RATE`], the official `resample_linear`.
/// A shell that already records at 16 kHz never enters this.
pub fn resample_linear(input: &[f32], sr_in: usize, sr_out: usize) -> Vec<f32> {
    if input.is_empty() || sr_in == 0 || sr_out == 0 {
        return Vec::new();
    }
    if sr_in == sr_out {
        return input.to_vec();
    }
    let ratio = sr_in as f64 / sr_out as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let i0 = src.floor() as usize;
        if i0 >= input.len() {
            break;
        }
        let frac = (src - i0 as f64) as f32;
        let s0 = input[i0];
        let s1 = if i0 + 1 < input.len() {
            input[i0 + 1]
        } else {
            s0
        };
        out.push(s0 * (1.0 - frac) + s1 * frac);
    }
    out
}

/// PCM16 at the shell's rate → the detector's 16 kHz f32 samples. The model
/// itself has no rate parameter; this is the one place a non-16 kHz shell
/// post is reconciled.
pub fn pcm16_at_rate(samples: &[i16], rate: u32) -> Vec<f32> {
    let mono = i16_to_f32(samples);
    if rate as usize == SAMPLE_RATE || rate == 0 {
        mono
    } else {
        resample_linear(&mono, rate as usize, SAMPLE_RATE)
    }
}

/// Standard-alphabet base64 decode. Whitespace is skipped; padding and an
/// alphabet-local length are enforced. Small on purpose: the core has no
/// base64 crate and the only payload that needs it is PCM.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    const PAD: u8 = 64;

    let mut symbols = Vec::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        symbols.push(if byte == b'=' { PAD } else { value(byte)? });
    }
    if symbols.len() % 4 != 0 {
        return None;
    }

    let chunks = symbols.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (index, chunk) in symbols.chunks(4).enumerate() {
        let last = index + 1 == chunks;
        let (a, b, c, d) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        if a == PAD || b == PAD {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        if c == PAD {
            // "AB==" is the one-byte tail; "AB=C" is not padding.
            if !last || d != PAD {
                return None;
            }
            continue;
        }
        out.push((b << 4) | (c >> 2));
        if d == PAD {
            // "ABC=" is the two-byte tail.
            if !last {
                return None;
            }
            continue;
        }
        out.push((c << 6) | d);
    }
    Some(out)
}

/// The `/sensor` `pcm16` payload: base64 of little-endian i16, mono.
pub fn decode_pcm16_base64(text: &str) -> Option<Vec<i16>> {
    let bytes = decode_base64(text)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    #[allow(clippy::chunks_exact_to_as_chunks)] // the alignment is the point
    Some(
        bytes
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model file the official upstream checkout ships. CI fetches
    /// `upstream/microduck`, and `DUCK_REQUIRE_FIXTURES=1` turns a missing
    /// one into a failure rather than a silent skip.
    fn model_path() -> Option<std::path::PathBuf> {
        let path = crate::fixtures::workspace_root()
            .join("upstream/microduck/pet-detect/models/pet_detect.onnx");
        if path.is_file() {
            return Some(path);
        }
        if crate::fixtures::required() {
            panic!("the official pet model is missing at {}", path.display());
        }
        eprintln!(
            "skipping: the official pet model is not here at {}",
            path.display()
        );
        None
    }

    /// The mel layout is the training contract; these numbers moving means
    /// retraining. Exactly the official `the_feature_contract_is_pinned`.
    #[test]
    fn the_feature_contract_is_pinned() {
        assert_eq!(SAMPLE_RATE, 16_000);
        assert_eq!(N_FFT, 512);
        assert_eq!(HOP, 160);
        assert_eq!(WIN, 400);
        assert_eq!(N_MELS, 40);
        assert_eq!(WINDOW_FRAMES, 100);
        assert_eq!(WINDOW_SAMPLES, 16_240);
        assert_eq!(N_MELS * WINDOW_FRAMES, 4_000);
        assert_eq!(DEFAULT_STRIDE, 4_060);
        assert_eq!((DEFAULT_ENTER, DEFAULT_EXIT), (0.95, 0.85));
        let filters = build_mel_filterbank(SAMPLE_RATE, N_FFT, N_MELS, FMIN, FMAX);
        assert_eq!(filters.len(), N_MELS);
        assert!(
            filters.iter().all(|filter| !filter.is_empty()),
            "every mel band must reach at least one FFT bin"
        );
        // 0 Hz is the first mel point and 8 kHz the last; the band edges must
        // land on those exact frequencies.
        assert!((hz_to_mel(FMIN) - 0.0).abs() < 1e-6);
        assert!((mel_to_hz(hz_to_mel(FMAX)) - FMAX).abs() < 1e-3);
    }

    /// The hand-written FFT against the definition, on a deterministic
    /// signal. This is the one place a transform bug would hide: a scrambled
    /// butterfly still produces plausible-looking logs.
    #[test]
    fn the_fft_matches_a_direct_dft() {
        let mut samples = [0.0f32; N_FFT];
        let mut state = 0x1234_5678u32;
        for (index, sample) in samples.iter_mut().enumerate() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 8) as f32 / (1 << 24) as f32 - 0.5;
            let angle = 2.0 * PI * 37.0 * index as f32 / N_FFT as f32;
            *sample = angle.sin() * 0.7 + noise * 0.1;
        }

        let twiddles: Vec<Complex> = (0..N_FFT / 2)
            .map(|k| {
                let angle = -2.0 * PI * k as f32 / N_FFT as f32;
                Complex::new(angle.cos(), angle.sin())
            })
            .collect();
        let mut got: Vec<Complex> = samples.iter().map(|&s| Complex::new(s, 0.0)).collect();
        fft_forward(&mut got, &twiddles);

        let mut worst = 0.0f32;
        for (bin, value) in got.iter().enumerate() {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (n, &sample) in samples.iter().enumerate() {
                let angle = -2.0 * std::f64::consts::PI * (bin * n) as f64 / N_FFT as f64;
                re += sample as f64 * angle.cos();
                im += sample as f64 * angle.sin();
            }
            worst = worst.max((value.re as f64 - re).abs() as f32);
            worst = worst.max((value.im as f64 - im).abs() as f32);
        }
        assert!(
            worst < 1e-2,
            "radix-2 FFT differs from the direct DFT by {worst}"
        );
    }

    /// Silence is the log floor; a tone lights the mel bands around its
    /// frequency. The "vector" here is the filterbank's own response, not an
    /// invented official recording.
    #[test]
    fn silence_is_the_log_floor_and_a_tone_lights_its_band() {
        let mut extractor = MelExtractor::new();
        let silence = vec![0.0f32; WINDOW_SAMPLES];
        let quiet = extractor.log_mel(&silence);
        assert!(
            quiet
                .iter()
                .all(|&value| (value - LOG_EPS.ln()).abs() < 1e-3)
        );

        let tone: Vec<f32> = (0..WINDOW_SAMPLES)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin())
            .collect();
        let loud = extractor.log_mel(&tone);
        assert!(loud.iter().cloned().fold(f32::MIN, f32::max) > 0.0);

        // Average each band over the 100 frames. 440 Hz sits in the low
        // bands: with 40 mel bands over 0..8 kHz, hz_to_mel(440) is ~549 of
        // the ~2840 mel span, i.e. band 6. Assert a window around it rather
        // than pretending to more precision than a triangle filter has.
        let band_energy: Vec<f32> = (0..N_MELS)
            .map(|m| {
                (0..WINDOW_FRAMES)
                    .map(|f| loud[m * WINDOW_FRAMES + f])
                    .sum()
            })
            .collect();
        let peak = band_energy
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index)
            .unwrap();
        assert!(
            (5..=7).contains(&peak),
            "440 Hz should peak in mel band 6, got {peak}"
        );
        assert!(
            band_energy[peak] > band_energy[N_MELS - 1] + 1.0,
            "the tone must not look like broadband noise"
        );
    }

    #[test]
    fn pcm16_decodes_little_endian_and_refuses_junk() {
        // Two samples: 1 and -2, LE: 01 00 fe ff.
        let encoded = "AQD+/w==";
        assert_eq!(decode_pcm16_base64(encoded), Some(vec![1, -2]));
        // An odd byte count cannot be i16.
        assert_eq!(decode_pcm16_base64("AQAe"), None);
        assert_eq!(decode_pcm16_base64("!!!!"), None);
        // Whitespace is tolerated; `AQA=` is a two-byte tail, one sample.
        assert_eq!(decode_pcm16_base64("AQA="), Some(vec![1]));
        assert_eq!(decode_pcm16_base64("AQA=\n"), Some(vec![1]));
        assert_eq!(
            decode_pcm16_base64("A=A="),
            None,
            "padding cannot be interior"
        );
        assert_eq!(
            decode_base64(" A Q A e / w = = "),
            Some(vec![1, 0, 0x1e, 0xff])
        );
    }

    #[test]
    fn resampling_a_second_of_audio_keeps_a_second_of_audio() {
        let tone: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        let down = resample_linear(&tone, 48_000, 16_000);
        assert!((down.len() as i64 - 15_999).abs() <= 1);
        let pcm = pcm16_at_rate(
            &tone
                .iter()
                .map(|v| (v * 32767.0) as i16)
                .collect::<Vec<_>>(),
            48_000,
        );
        assert!((pcm.len() as i64 - 15_999).abs() <= 1);
    }

    /// The official hysteresis, driven by probabilities because a real
    /// scratch recording is not something this repository ships.
    #[test]
    fn the_hysteresis_is_enter_95_exit_85() {
        let mut detector = match model_path() {
            Some(path) => PettingDetector::load(&path).expect("the official pet model loads"),
            None => return,
        };
        assert_eq!(detector.hysteresis(0.70), None);
        assert_eq!(detector.hysteresis(0.94), None, "below enter");
        assert_eq!(detector.hysteresis(0.95), Some(PettingEvent::Start));
        assert!(detector.is_petting());
        // A held high score is not a stream of Starts.
        for _ in 0..10 {
            assert_eq!(detector.hysteresis(0.99), None);
        }
        assert_eq!(detector.hysteresis(0.86), None, "still above exit");
        assert_eq!(detector.hysteresis(0.84), Some(PettingEvent::End));
        assert!(!detector.is_petting());
        // Crossing the enter threshold again is a new pet, not the old one.
        assert_eq!(detector.hysteresis(0.96), Some(PettingEvent::Start));
    }

    /// The window has to fill before the model runs, and then it runs once
    /// per stride — the official schedule, measured rather than assumed.
    #[test]
    fn the_detector_waits_for_a_full_window_then_runs_on_the_stride() {
        let Some(path) = model_path() else {
            return;
        };
        let mut detector = PettingDetector::load(&path).expect("the official pet model loads");
        let below = WINDOW_SAMPLES - 1;
        detector
            .push_samples(&vec![0.0; below], true)
            .expect("a partial window is buffered, not inferred");
        assert_eq!(detector.inferences(), 0, "no model run before a window");
        assert_eq!(detector.buffered_samples(), below);

        detector
            .push_samples(&[0.0], true)
            .expect("the first full window runs");
        assert_eq!(detector.inferences(), 1);

        // samples_until_infer advanced to WINDOW_SAMPLES + stride; only a
        // push that reaches it can run a second window.
        detector
            .push_samples(&vec![0.0; DEFAULT_STRIDE - 1], true)
            .expect("buffered");
        assert_eq!(detector.inferences(), 1);
        detector.push_samples(&[0.0], true).expect("second window");
        assert_eq!(detector.inferences(), 2);
    }

    /// A model that is not there is an ordinary error: the caller turns it
    /// into "feature off", never a panic and never a broken startup.
    #[test]
    fn a_missing_model_is_an_error_not_a_panic() {
        let path = std::env::temp_dir().join("duck-no-such-pet-model.onnx");
        let _ = std::fs::remove_file(&path);
        assert!(PettingDetector::load(&path).is_err());
    }

    /// `audible = false` while not petting skips inference but keeps the
    /// ring and schedule continuous — the official gate.
    #[test]
    fn silence_skips_inference_until_petting_or_audible() {
        let Some(path) = model_path() else {
            return;
        };
        let mut detector = PettingDetector::load(&path).expect("the official pet model loads");
        detector
            .push_samples(&vec![0.0; WINDOW_SAMPLES], false)
            .expect("a quiet window is skipped");
        assert_eq!(detector.inferences(), 0);
        assert!(detector.buffered_samples() >= WINDOW_SAMPLES);
        detector
            .push_samples(&vec![0.0; DEFAULT_STRIDE], false)
            .expect("still quiet");
        assert_eq!(detector.inferences(), 0);
        detector
            .push_samples(&vec![0.0; DEFAULT_STRIDE], true)
            .expect("the gate opened");
        assert_eq!(detector.inferences(), 1);
    }
}
