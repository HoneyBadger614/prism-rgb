//! Readings for the effects that show what the computer is doing: processor
//! and graphics card temperatures, and the music playing. Each is only read
//! while a layer shows it.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use prism_core::effects::{Sensors, BANDS};

/// A background reader; it stops when dropped.
struct Worker {
    stop: Arc<AtomicBool>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn spawn(readings: &Arc<Mutex<Sensors>>, run: fn(&AtomicBool, &Mutex<Sensors>)) -> Worker {
    let stop = Arc::new(AtomicBool::new(false));
    let (s, r) = (stop.clone(), readings.clone());
    thread::spawn(move || run(&s, &r));
    Worker { stop }
}

/// The latest readings, and the readers that are running.
pub struct SensorHub {
    readings: Arc<Mutex<Sensors>>,
    temps: Option<Worker>,
    music: Option<Worker>,
}

impl SensorHub {
    pub fn new() -> Self {
        Self { readings: Arc::new(Mutex::new(Sensors::default())), temps: None, music: None }
    }

    /// Starts or stops the readers, for what the layers show now.
    pub fn want(&mut self, temps: bool, music: bool) {
        if temps != self.temps.is_some() {
            self.temps = temps.then(|| spawn(&self.readings, read_temps));
        }
        if music != self.music.is_some() {
            self.music = music.then(|| spawn(&self.readings, listen));
            if !music {
                self.readings.lock().unwrap().bands = [0.0; BANDS];
            }
        }
    }

    pub fn read(&self) -> Sensors {
        *self.readings.lock().unwrap()
    }
}

/* ---------- temperatures ---------- */

fn read_temps(stop: &AtomicBool, readings: &Mutex<Sensors>) {
    // Ask nvidia-smi only if there's an NVIDIA card and no other reading.
    let mut nvidia = true;
    while !stop.load(Ordering::Relaxed) {
        let cpu = cpu_temp();
        let mut gpu = hwmon_temp(&["amdgpu", "radeon", "nouveau"], &["edge"]);
        if gpu.is_none() && nvidia {
            gpu = nvidia_temp();
            nvidia = gpu.is_some();
        }
        {
            let mut r = readings.lock().unwrap();
            r.cpu_temp = cpu;
            r.gpu_temp = gpu;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn cpu_temp() -> Option<f32> {
    hwmon_temp(&["k10temp", "zenpower", "coretemp", "cpu_thermal"], &["Tctl", "Tdie", "Package id 0"])
}

/// A temperature from the first hwmon sensor with one of `names`: the input
/// labelled with one of `labels` if there is one, otherwise its first.
fn hwmon_temp(names: &[&str], labels: &[&str]) -> Option<f32> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    for dir in std::fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|e| e.path()) {
        if !read(dir.join("name")).is_some_and(|n| names.contains(&n.as_str())) {
            continue;
        }
        let labelled = (1..10).find(|i| read(dir.join(format!("temp{i}_label"))).is_some_and(|l| labels.contains(&l.as_str())));
        let milli: f32 = read(dir.join(format!("temp{}_input", labelled.unwrap_or(1))))?.parse().ok()?;
        return Some(milli / 1000.0);
    }
    None
}

fn nvidia_temp() -> Option<f32> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next()?.trim().parse().ok()
}

/* ---------- music ---------- */

const RATE: f32 = 48_000.0;
/// Samples per look at the music (about 43 ms), taken every [`HOP`].
const SIZE: usize = 2048;
const HOP: usize = 1024;

/// Records what's playing (the default output's monitor) as 16-bit mono.
fn record() -> Option<Child> {
    Command::new("parec")
        .args(["--device=@DEFAULT_MONITOR@", "--format=s16le", "--rate=48000", "--channels=1", "--raw", "--latency-msec=20"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

fn listen(stop: &AtomicBool, readings: &Mutex<Sensors>) {
    let Some(mut child) = record() else {
        eprintln!("prism: can't hear the music: parec (PulseAudio or PipeWire's pulse tools) isn't installed");
        return;
    };
    let mut out = child.stdout.take().unwrap();
    let mut samples = vec![0.0f32; SIZE];
    let mut bytes = vec![0u8; HOP * 2];
    let mut bands = Bands::new();
    while !stop.load(Ordering::Relaxed) && out.read_exact(&mut bytes).is_ok() {
        samples.copy_within(HOP.., 0);
        for (s, b) in samples[SIZE - HOP..].iter_mut().zip(bytes.chunks_exact(2)) {
            *s = i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0;
        }
        readings.lock().unwrap().bands = bands.update(&samples);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Turns sound into how loud each band is, 0-1, adjusting to the volume.
struct Bands {
    /// Which FFT bins each band covers.
    bins: Vec<std::ops::Range<usize>>,
    /// The loudest each band has been lately, in dB.
    peak: [f32; BANDS],
    /// What's showing, falling back gently.
    shown: [f32; BANDS],
}

impl Bands {
    fn new() -> Self {
        // Log-spaced from 40 Hz to 16 kHz, at least one bin each.
        let hz = |i: usize| 40.0 * 400f32.powf(i as f32 / BANDS as f32);
        let bin = |f: f32| (f / (RATE / SIZE as f32)) as usize;
        let bins = (0..BANDS).map(|i| bin(hz(i)).max(1)..bin(hz(i + 1)).max(bin(hz(i)).max(1) + 1)).collect();
        Self { bins, peak: [-35.0; BANDS], shown: [0.0; BANDS] }
    }

    fn update(&mut self, samples: &[f32]) -> [f32; BANDS] {
        // Hann window, so a band doesn't leak into its neighbours.
        let n = samples.len();
        let mut re: Vec<f32> = samples.iter().enumerate().map(|(i, s)| s * (std::f32::consts::PI * i as f32 / n as f32).sin().powi(2)).collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        // A full-scale sine comes out near 0 dB.
        let scale = (n as f32 / 4.0).powi(2);
        for (b, bins) in self.bins.iter().enumerate() {
            let power = bins.clone().map(|k| (re[k] * re[k] + im[k] * im[k]) / scale).fold(0.0, f32::max);
            let db = 10.0 * (power + 1e-12).log10();
            // Each band adjusts to its own loudest lately (treble is
            // quieter than bass), but quiet noise stays dark.
            self.peak[b] = db.max(self.peak[b] - 0.03).max(-35.0);
            let v = ((db - self.peak[b] + 30.0) / 30.0).clamp(0.0, 1.0);
            self.shown[b] = v.max(self.shown[b] - 0.04);
        }
        self.shown
    }
}

/// In-place fast Fourier transform; the length must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
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
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((angle * k as f32).cos(), (angle * k as f32).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bass_note_lights_the_bass_bands() {
        let mut bands = Bands::new();
        let tone: Vec<f32> = (0..SIZE).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / RATE).sin()).collect();
        let b = bands.update(&tone);
        assert!(b[1] > 0.8, "{b:?}");
        assert!(b[BANDS - 1] < 0.1, "{b:?}");
        // Silence fades out.
        let quiet = vec![0.0; SIZE];
        let b = (0..40).map(|_| bands.update(&quiet)).last().unwrap();
        assert!(b.iter().all(|&v| v == 0.0), "{b:?}");
    }

    #[test]
    fn reads_this_computers_temperatures_if_any() {
        // Just mustn't panic; the numbers depend on the machine.
        if let Some(t) = cpu_temp() {
            assert!((0.0..150.0).contains(&t), "{t}");
        }
    }
}
