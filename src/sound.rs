//! Islet's own notification chimes, synthesized once into in-memory WAV files.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
pub enum Chime {
    /// Two rising bell notes: reminders, timers, water.
    Soft,
    /// Three short blips: low battery.
    Alert,
}

/// (start s, frequency Hz, decay per s) for each note.
fn notes(c: Chime) -> &'static [(f32, f32, f32)] {
    match c {
        Chime::Soft => &[(0.0, 784.0, 7.0), (0.13, 1174.7, 5.5)],
        Chime::Alert => &[(0.0, 987.8, 22.0), (0.12, 987.8, 22.0), (0.24, 987.8, 22.0)],
    }
}

fn synth(c: Chime) -> Vec<u8> {
    let len = (RATE as f32 * 0.9) as usize;
    let mut buf = vec![0f32; len];
    for &(start, f, decay) in notes(c) {
        let s0 = (start * RATE as f32) as usize;
        for (i, v) in buf.iter_mut().enumerate().skip(s0) {
            let t = (i - s0) as f32 / RATE as f32;
            let attack = (t / 0.004).min(1.0);
            let env = attack * (-t * decay).exp();
            // A bell-ish partial mix: fundamental plus a soft octave.
            *v += env * ((TAU * f * t).sin() + 0.25 * (TAU * 2.0 * f * t).sin());
        }
    }
    let pcm: Vec<i16> = buf.iter().map(|v| (v * 0.28).clamp(-1.0, 1.0)).map(|v| (v * i16::MAX as f32) as i16).collect();
    let data = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    pcm.iter().for_each(|s| wav.extend_from_slice(&s.to_le_bytes()));
    wav
}

pub fn play(c: Chime) {
    static SOFT: OnceLock<Vec<u8>> = OnceLock::new();
    static ALERT: OnceLock<Vec<u8>> = OnceLock::new();
    let wav = match c {
        Chime::Soft => SOFT.get_or_init(|| synth(c)),
        Chime::Alert => ALERT.get_or_init(|| synth(c)),
    };
    // The buffers are static, so they outlive the asynchronous playback.
    unsafe {
        let _ = PlaySoundW(PCWSTR(wav.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}
