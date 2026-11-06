//! What is playing, heard for real: a loopback capture of the default output, cut by an FFT into
//! five bands from bass to treble, for the equalizer bars and the eyes'
//! beat. It only listens while something needs it; otherwise the capture is let go.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

/// Samples per FFT, and how far it moves on between two.
const N: usize = 1024;
const HOP: usize = 512;
/// The bands, in Hz: bass, low mids, mids, upper mids, treble.
pub const BANDS: [(f32, f32); 5] = [(40.0, 150.0), (150.0, 400.0), (400.0, 1000.0), (1000.0, 3000.0), (3000.0, 10000.0)];
/// How quickly each band's own average follows it (per analysis, about half a second).
const SETTLE: f32 = 0.03;
/// While anything sounds, a bar never lies completely flat.
const FLOOR: f32 = 0.12;
/// A bass beat: this much above its recent average, and not again for this many analyses.
const BEAT_OVER: f32 = 1.35;
const BEAT_GAP: u32 = 8;

static WANT: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
static SHARED: Mutex<Shared> = Mutex::new(Shared { bands: None });

struct Shared {
    bands: Option<[f32; 5]>,
}

/// Ask for the sound to be listened to (true) or let go (false). Cheap to call every frame.
pub fn want(on: bool) {
    if WANT.swap(on, Ordering::Relaxed) != on {
        crate::sys::timelog(if on { "spectrum on" } else { "spectrum off" });
    }
    if on && !STARTED.swap(true, Ordering::Relaxed) {
        std::thread::spawn(run);
    }
}

/// The five band levels (0..1), bass first; None while nothing has been heard.
pub fn bands() -> Option<[f32; 5]> {
    if !WANT.load(Ordering::Relaxed) {
        return None;
    }
    SHARED.lock().ok()?.bands
}


fn run() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    loop {
        if !WANT.load(Ordering::Relaxed) {
            if let Ok(mut s) = SHARED.lock() {
                s.bands = None;
            }
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        if listen().is_err() {
            crate::sys::timelog("spectrum: capture failed, trying again soon");
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Captures until it is no longer wanted (Ok) or the device goes away (Err).
fn listen() -> windows::core::Result<()> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (rate, channels, bits) = ((*fmt).nSamplesPerSec as f32, (*fmt).nChannels as usize, (*fmt).wBitsPerSample);
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 200_000, 0, fmt as *const WAVEFORMATEX, None);
        CoTaskMemFree(Some(fmt as *const _));
        init?;
        // The shared-mode mix is 32-bit float; anything else is left to the fallback.
        if bits != 32 || channels == 0 {
            return Err(windows::core::Error::empty());
        }
        let capture: IAudioClient = client.clone();
        let cap: IAudioCaptureClient = capture.GetService()?;
        client.Start()?;
        let mut an = Analyser::new(rate);
        let mut mono: Vec<f32> = Vec::with_capacity(N * 2);
        let mut quiet_since = Instant::now();
        while WANT.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
            let mut got = false;
            while cap.GetNextPacketSize()? > 0 {
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = flags & 2 != 0;
                let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * channels);
                for f in 0..frames as usize {
                    let v = if silent { 0.0 } else { samples[f * channels..(f + 1) * channels].iter().sum::<f32>() / channels as f32 };
                    mono.push(v);
                }
                cap.ReleaseBuffer(frames)?;
                got = true;
            }
            while mono.len() >= N {
                let (bands, _) = an.step(&mono[..N]);
                if let Ok(mut s) = SHARED.lock() {
                    s.bands = Some(bands);
                }
                mono.drain(..HOP);
            }
            // Loopback sends nothing while the output is silent: the bars settle down.
            if got {
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() > Duration::from_millis(150) {
                let bands = an.step(&[0.0; N]).0;
                if let Ok(mut s) = SHARED.lock() {
                    s.bands = Some(bands);
                }
            }
        }
        let _ = client.Stop();
        Ok(())
    }
}

/// Turns blocks of samples into five smoothed, self-levelling band heights and bass beats.
pub struct Analyser {
    rate: f32,
    window: Vec<f32>,
    /// Each band's recent average: a bar shows how far the band stands above it.
    avg: [f32; 5],
    /// The loudest the whole sound has been lately (decaying): below a hundredth of it is silence.
    peak: f32,
    bass_avg: f32,
    cooling: u32,
}

impl Analyser {
    pub fn new(rate: f32) -> Self {
        let window = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (N - 1) as f32).cos()).collect();
        Analyser { rate, window, avg: [0.0; 5], peak: 1e-6, bass_avg: 0.0, cooling: 0 }
    }

    /// One block of N samples: the band heights (0..1) and whether a bass beat landed. Each bar
    /// shows how far its band stands above its own recent average, so a kick lifts the bass, a
    /// steady sound sits in the middle, and the five move each to their own beat. The drawing
    /// smooths them.
    pub fn step(&mut self, block: &[f32]) -> ([f32; 5], bool) {
        let raw = band_energy(block, &self.window, self.rate);
        let total: f32 = raw.iter().sum();
        self.peak = (self.peak * 0.998).max(total).max(1e-6);
        let sounding = total > self.peak * 0.01 && total > 1e-5;
        let mut out = [0.0; 5];
        for i in 0..5 {
            let avg = self.avg[i].max(1e-7);
            let r = raw[i] / avg;
            self.avg[i] += (raw[i] - self.avg[i]) * SETTLE;
            out[i] = if sounding { FLOOR + (1.0 - FLOOR) * ((r - 0.7) / 1.3).clamp(0.0, 1.0) } else { 0.0 };
        }
        let bass = raw[0];
        let beat = self.cooling == 0 && bass > 1e-4 && bass > self.bass_avg * BEAT_OVER;
        self.bass_avg += (bass - self.bass_avg) * 0.1;
        self.cooling = if beat { BEAT_GAP } else { self.cooling.saturating_sub(1) };
        (out, beat)
    }
}

/// The energy in each of the five bands of a windowed block.
pub fn band_energy(block: &[f32], window: &[f32], rate: f32) -> [f32; 5] {
    let mut re: Vec<f32> = block.iter().zip(window).map(|(s, w)| s * w).collect();
    let mut im = vec![0.0; re.len()];
    fft(&mut re, &mut im);
    let bin = rate / re.len() as f32;
    let mut out = [0.0; 5];
    for (b, (lo, hi)) in BANDS.iter().enumerate() {
        let (a, z) = (((lo / bin) as usize).max(1), ((hi / bin) as usize).min(re.len() / 2));
        let mut sum = 0.0;
        for k in a..z.max(a + 1) {
            sum += re[k] * re[k] + im[k] * im[k];
        }
        out[b] = (sum / (z.max(a + 1) - a) as f32).sqrt();
    }
    out
}

/// In-place radix-2 FFT; the length must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: f32) -> Vec<f32> {
        (0..N).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect()
    }

    fn loudest(e: [f32; 5]) -> usize {
        (0..5).max_by(|a, b| e[*a].partial_cmp(&e[*b]).unwrap()).unwrap()
    }

    #[test]
    fn a_low_tone_lands_in_the_bass_and_a_high_one_in_the_treble() {
        let an = Analyser::new(48_000.0);
        assert_eq!(loudest(band_energy(&tone(90.0, 48_000.0), &an.window, 48_000.0)), 0);
        assert_eq!(loudest(band_energy(&tone(600.0, 48_000.0), &an.window, 48_000.0)), 2);
        assert_eq!(loudest(band_energy(&tone(5_000.0, 48_000.0), &an.window, 48_000.0)), 4);
    }

    #[test]
    fn a_kick_lifts_the_bass_and_a_steady_sound_settles_mid_way() {
        let mut an = Analyser::new(48_000.0);
        let silence = vec![0.0; N];
        let kick = tone(80.0, 48_000.0);
        for _ in 0..20 {
            assert_eq!(an.step(&silence).0, [0.0; 5], "silence lies flat");
        }
        let (up, beat) = an.step(&kick);
        assert!(up[0] > 0.9, "a kick out of quiet lifts the bass: {}", up[0]);
        assert!(beat, "and is a beat");
        assert!(!an.step(&kick).1, "not twice in a row");
        let mut steady = [0.0; 5];
        for _ in 0..400 {
            steady = an.step(&kick).0;
        }
        assert!(steady[0] > 0.2 && steady[0] < 0.6, "a steady sound sits mid-way: {}", steady[0]);
    }
}
