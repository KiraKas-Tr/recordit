use anyhow::{Context, Result, bail};
use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
use hound::{SampleFormat, WavSpec, WavWriter};
use screencapturekit::prelude::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct TimedChunk {
    kind: SCStreamOutputType,
    pts_seconds: f64,
    sample_rate_hz: u32,
    mono_samples: Vec<f32>,
}

fn parse_u64_arg(args: &[String], index: usize, default: u64) -> Result<u64> {
    if let Some(value) = args.get(index) {
        return value
            .parse::<u64>()
            .with_context(|| format!("argument {index} must be an integer"));
    }
    Ok(default)
}

fn parse_output_arg(args: &[String], index: usize, default: &str) -> PathBuf {
    args.get(index)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    out
}

fn downmix_to_mono(sample: &CMSampleBuffer) -> Result<Vec<f32>> {
    let list = sample
        .audio_buffer_list()
        .ok_or_else(|| anyhow::anyhow!("audio sample had no AudioBufferList"))?;

    if list.num_buffers() == 0 {
        return Ok(Vec::new());
    }

    if list.num_buffers() == 1 {
        let buffer = list
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("missing first audio buffer"))?;
        let channels = usize::max(buffer.number_channels as usize, 1);
        let samples = bytes_to_f32(buffer.data());

        if channels == 1 {
            return Ok(samples);
        }

        let frames = samples.len() / channels;
        let mut mono = Vec::with_capacity(frames);
        for frame in 0..frames {
            let mut acc = 0.0f32;
            for ch in 0..channels {
                acc += samples[frame * channels + ch];
            }
            mono.push(acc / channels as f32);
        }
        return Ok(mono);
    }

    let mut channel_data = Vec::with_capacity(list.num_buffers());
    let mut min_frames = usize::MAX;

    for buffer in &list {
        let samples = bytes_to_f32(buffer.data());
        min_frames = min_frames.min(samples.len());
        channel_data.push(samples);
    }

    if min_frames == usize::MAX || min_frames == 0 {
        return Ok(Vec::new());
    }

    let mut mono = vec![0.0f32; min_frames];
    let scale = 1.0f32 / channel_data.len() as f32;
    for channel in &channel_data {
        for i in 0..min_frames {
            mono[i] += channel[i] * scale;
        }
    }

    Ok(mono)
}

fn sample_to_chunk(sample: CMSampleBuffer, kind: SCStreamOutputType) -> Result<TimedChunk> {
    let pts_seconds = sample
        .presentation_timestamp()
        .as_seconds()
        .unwrap_or_default();
    let format = sample
        .format_description()
        .ok_or_else(|| anyhow::anyhow!("missing format description"))?;

    if !format.audio_is_float() {
        bail!("audio sample is not float PCM");
    }

    let sample_rate_hz = format
        .audio_sample_rate()
        .ok_or_else(|| anyhow::anyhow!("missing audio sample rate"))?
        .round() as u32;

    let mono_samples = downmix_to_mono(&sample)?;

    Ok(TimedChunk {
        kind,
        pts_seconds,
        sample_rate_hz,
        mono_samples,
    })
}

fn callback(sender: &Sender<TimedChunk>, sample: CMSampleBuffer, kind: SCStreamOutputType) {
    if let Ok(chunk) = sample_to_chunk(sample, kind) {
        let _ = sender.try_send(chunk);
    }
}

fn paint_chunks_timeline(chunks: &[TimedChunk], base_pts: f64, sample_rate_hz: u32) -> Vec<f32> {
    let mut timeline = Vec::<f32>::new();
    let rate = f64::from(sample_rate_hz);

    for chunk in chunks {
        let start = ((chunk.pts_seconds - base_pts) * rate).round();
        let start_index = if start <= 0.0 { 0usize } else { start as usize };
        let end_index = start_index.saturating_add(chunk.mono_samples.len());
        if timeline.len() < end_index {
            timeline.resize(end_index, 0.0);
        }
        timeline[start_index..end_index].copy_from_slice(&chunk.mono_samples);
    }

    timeline
}

fn write_interleaved_stereo_wav(
    path: &Path,
    sample_rate_hz: u32,
    mic: &[f32],
    sys: &[f32],
) -> Result<()> {
    let spec = WavSpec {
        channels: 2,
        sample_rate: sample_rate_hz,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec).context("failed to create WAV writer")?;
    let frame_count = mic.len().max(sys.len());

    for i in 0..frame_count {
        let left = mic.get(i).copied().unwrap_or(0.0);
        let right = sys.get(i).copied().unwrap_or(0.0);
        writer
            .write_sample(left)
            .context("failed to write mic sample")?;
        writer
            .write_sample(right)
            .context("failed to write system sample")?;
    }

    writer.finalize().context("failed to finalize WAV file")?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let duration_secs = parse_u64_arg(&args, 1, 10)?;
    let output = parse_output_arg(&args, 2, "artifacts/hello-world.wav");
    let target_rate_hz = parse_u64_arg(&args, 3, 48_000)? as u32;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    println!(
        "Starting Sequoia capture for {}s -> {}",
        duration_secs,
        output.display()
    );
    println!("Stereo mapping: left=mic, right=system");

    let content = SCShareableContent::get().context(
        "failed to get shareable content (screen recording permission + active display required)",
    )?;
    let displays = content.displays();
    if displays.is_empty() {
        bail!("no displays available for SCContentFilter");
    }

    let filter = SCContentFilter::create()
        .with_display(&displays[0])
        .with_excluding_windows(&[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_captures_microphone(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(target_rate_hz as i32)
        .with_channel_count(2);

    let queue = DispatchQueue::new("com.sequoia-capture.recorder", DispatchQoS::UserInteractive);
    let (tx, rx) = bounded::<TimedChunk>(4_096);
    let mut stream = SCStream::new(&filter, &config);

    let tx_audio = tx.clone();
    stream
        .add_output_handler_with_queue(
            move |sample, kind| callback(&tx_audio, sample, kind),
            SCStreamOutputType::Audio,
            Some(&queue),
        )
        .ok_or_else(|| anyhow::anyhow!("failed to add system-audio handler"))?;

    let tx_mic = tx.clone();
    stream
        .add_output_handler_with_queue(
            move |sample, kind| callback(&tx_mic, sample, kind),
            SCStreamOutputType::Microphone,
            Some(&queue),
        )
        .ok_or_else(|| anyhow::anyhow!("failed to add microphone handler"))?;

    stream
        .start_capture()
        .context("failed to start stream capture")?;

    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let mut mic_chunks = Vec::<TimedChunk>::new();
    let mut sys_chunks = Vec::<TimedChunk>::new();

    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => match chunk.kind {
                SCStreamOutputType::Audio => sys_chunks.push(chunk),
                SCStreamOutputType::Microphone => mic_chunks.push(chunk),
                SCStreamOutputType::Screen => {}
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    stream
        .stop_capture()
        .context("failed to stop stream capture")?;

    if mic_chunks.is_empty() || sys_chunks.is_empty() {
        bail!(
            "missing captured data (mic chunks: {}, system chunks: {})",
            mic_chunks.len(),
            sys_chunks.len()
        );
    }

    let mic_rate = mic_chunks[0].sample_rate_hz;
    let sys_rate = sys_chunks[0].sample_rate_hz;

    if mic_rate != target_rate_hz || sys_rate != target_rate_hz {
        bail!(
            "unexpected sample rates (mic={} Hz, system={} Hz, target={} Hz); resampling not implemented in prototype",
            mic_rate,
            sys_rate,
            target_rate_hz
        );
    }

    let base_pts = mic_chunks[0].pts_seconds.min(sys_chunks[0].pts_seconds);
    let mic = paint_chunks_timeline(&mic_chunks, base_pts, target_rate_hz);
    let sys = paint_chunks_timeline(&sys_chunks, base_pts, target_rate_hz);
    write_interleaved_stereo_wav(&output, target_rate_hz, &mic, &sys)?;

    println!(
        "WAV written: {} (mic chunks: {}, system chunks: {}, frames: {})",
        output.display(),
        mic_chunks.len(),
        sys_chunks.len(),
        mic.len().max(sys.len())
    );

    Ok(())
}
