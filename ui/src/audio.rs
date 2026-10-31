use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub fn devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|ds| {
            ds.filter_map(|d| d.description().ok().map(|x| x.name().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub enum Source {
    Device(Device),
    File(WavFile),
    Host(HostInput),
}

pub struct HostInput {
    pub(crate) name: String,
    pub(crate) sample_rate: u32,
    pub(crate) buffer: Vec<f32>,
}

impl Source {
    pub fn sample_rate(&self) -> u32 {
        match self {
            Source::Device(d) => d.sample_rate,
            Source::File(f) => f.sample_rate,
            Source::Host(h) => h.sample_rate,
        }
    }

    pub fn take(&mut self) -> Vec<f32> {
        match self {
            Source::Device(d) => d.rx.try_iter().flatten().collect(),
            Source::File(f) => f.take(),
            Source::Host(h) => std::mem::take(&mut h.buffer),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Source::Device(d) => &d.name,
            Source::File(f) => &f.name,
            Source::Host(h) => &h.name,
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self, Source::File(f) if f.pos >= f.samples.len())
    }
}

pub struct Device {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    sample_rate: u32,
    name: String,
}

impl Device {
    pub fn open(name: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match name {
            Some(n) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.description().map(|x| x.name() == n).unwrap_or(false))
                .ok_or_else(|| format!("no input device {n}"))?,
            None => host.default_input_device().ok_or("no input device")?,
        };
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let sample_rate = config.sample_rate();
        let (tx, rx) = channel();
        let err = |e| eprintln!("audio input: {e}");
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _: &_| {
                    let _ = tx.send(data.chunks(channels).map(|f| f[0]).collect());
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _: &_| {
                    let _ = tx.send(
                        data.chunks(channels)
                            .map(|f| f[0] as f32 / 32768.0)
                            .collect(),
                    );
                },
                err,
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream(
                stream_config,
                move |data: &[i32], _: &_| {
                    let _ = tx.send(
                        data.chunks(channels)
                            .map(|f| f[0] as f32 / 2147483648.0)
                            .collect(),
                    );
                },
                err,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let name = device
            .description()
            .map(|x| x.name().to_string())
            .unwrap_or_default();
        Ok(Device {
            _stream: stream,
            rx,
            sample_rate,
            name,
        })
    }
}

pub struct WavFile {
    samples: Vec<f32>,
    sample_rate: u32,
    pos: usize,
    started: Instant,
    name: String,
}

impl WavFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let (samples, sample_rate) = read_wav(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(WavFile {
            samples,
            sample_rate,
            pos: 0,
            started: Instant::now(),
            name,
        })
    }

    fn take(&mut self) -> Vec<f32> {
        let due = (self.started.elapsed().as_secs_f64() * self.sample_rate as f64) as usize;
        let end = due.min(self.samples.len());
        let out = self.samples[self.pos.min(end)..end].to_vec();
        self.pos = end;
        out
    }
}

pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    let mono = all
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((mono, spec.sample_rate))
}
