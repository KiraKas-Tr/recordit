use anyhow::{bail, Context, Result};
use crossbeam_channel::RecvTimeoutError;
use recordit::rt_transport::{preallocated_spsc, PreallocatedProducer};
use screencapturekit::prelude::*;
use std::collections::VecDeque;
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct ProbeEvent {
    kind: SCStreamOutputType,
    pts_seconds: f64,
    duration_seconds: f64,
    num_samples: usize,
    sample_rate_hz: Option<f64>,
    channels: Option<u32>,
    bits_per_channel: Option<u32>,
    is_float: bool,
}

#[derive(Debug, Default)]
struct ProbeCallbackAudit {
    missing_format_description: AtomicU64,
}

impl ProbeEvent {
    fn empty() -> Self {
        Self {
            kind: SCStreamOutputType::Audio,
            pts_seconds: 0.0,
            duration_seconds: 0.0,
            num_samples: 0,
            sample_rate_hz: None,
            channels: None,
            bits_per_channel: None,
            is_float: false,
        }
    }
}

fn sample_to_event(sample: CMSampleBuffer, kind: SCStreamOutputType) -> ProbeEvent {
    let pts_seconds = sample
        .presentation_timestamp()
        .as_seconds()
        .unwrap_or_default();
    let duration_seconds = sample.duration().as_seconds().unwrap_or_default();
    let num_samples = sample.num_samples();

    let (sample_rate_hz, channels, bits_per_channel, is_float) =
        if let Some(fmt) = sample.format_description() {
            (
                fmt.audio_sample_rate(),
                fmt.audio_channel_count(),
                fmt.audio_bits_per_channel(),
                fmt.audio_is_float(),
            )
        } else {
            (None, None, None, false)
        };

    ProbeEvent {
        kind,
        pts_seconds,
        duration_seconds,
        num_samples,
        sample_rate_hz,
        channels,
        bits_per_channel,
        is_float,
    }
}

fn callback(
    producer: &PreallocatedProducer<ProbeEvent>,
    audit: &ProbeCallbackAudit,
    sample: CMSampleBuffer,
    kind: SCStreamOutputType,
) {
    if sample.format_description().is_none() {
        audit
            .missing_format_description
            .fetch_add(1, Ordering::Relaxed);
    }
    producer.try_push_with(|slot| {
        *slot = sample_to_event(sample, kind);
        true
    });
}

fn nearest_neighbor_deltas(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }

    let mut j = 0usize;
    let mut deltas = Vec::with_capacity(a.len().min(b.len()));

    for &av in a {
        while (j + 1) < b.len() && (b[j + 1] - av).abs() <= (b[j] - av).abs() {
            j += 1;
        }
        deltas.push(av - b[j]);
    }

    deltas
}

fn summarize_deltas(name: &str, deltas: &[f64]) {
    if deltas.is_empty() {
        println!("{name}: no comparable timestamps");
        return;
    }

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0f64;
    let mut abs_sum = 0.0f64;

    for &d in deltas {
        min = min.min(d);
        max = max.max(d);
        sum += d;
        abs_sum += d.abs();
    }

    let n = deltas.len() as f64;
    let mean = sum / n;
    let mean_abs = abs_sum / n;

    println!(
        "{name}: n={}, min={:.6} ms, max={:.6} ms, mean={:.6} ms, mean|d|={:.6} ms",
        deltas.len(),
        min * 1_000.0,
        max * 1_000.0,
        mean * 1_000.0,
        mean_abs * 1_000.0
    );
}

fn main() -> Result<()> {
    let secs = env::args()
        .nth(1)
        .map(|s| s.parse::<u64>())
        .transpose()
        .context("duration seconds must be an integer")?
        .unwrap_or(8);

    println!("Starting ScreenCaptureKit probe for {secs}s");
    println!("Expect first-run TCC prompts for Screen Recording and Microphone.");

    let content = SCShareableContent::get().context(
        "failed to get shareable content (check Screen Recording permission + active display)",
    )?;
    let displays = content.displays();
    if displays.is_empty() {
        bail!("no displays available for SCContentFilter");
    }
    let display = &displays[0];

    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&[])
        .build();

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_captures_microphone(true)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(48_000)
        .with_channel_count(2);

    let queue = DispatchQueue::new("com.sequoia-capture.probe", DispatchQoS::UserInteractive);
    let slots = (0..8_192).map(|_| ProbeEvent::empty()).collect();
    let (producer, consumer) = preallocated_spsc(slots);
    let callback_audit = Arc::new(ProbeCallbackAudit::default());

    let mut stream = SCStream::new(&filter, &config);

    let audio_producer = producer.clone();
    let audio_audit = Arc::clone(&callback_audit);
    stream
        .add_output_handler_with_queue(
            move |sample, kind| callback(&audio_producer, &audio_audit, sample, kind),
            SCStreamOutputType::Audio,
            Some(&queue),
        )
        .ok_or_else(|| anyhow::anyhow!("failed to add audio output handler"))?;

    let mic_producer = producer.clone();
    let mic_audit = Arc::clone(&callback_audit);
    stream
        .add_output_handler_with_queue(
            move |sample, kind| callback(&mic_producer, &mic_audit, sample, kind),
            SCStreamOutputType::Microphone,
            Some(&queue),
        )
        .ok_or_else(|| anyhow::anyhow!("failed to add microphone output handler"))?;

    stream
        .start_capture()
        .context("failed to start stream capture")?;

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut audio_pts = Vec::new();
    let mut mic_pts = Vec::new();
    let mut audio_seen = 0usize;
    let mut mic_seen = 0usize;
    let mut audio_printed = false;
    let mut mic_printed = false;
    let mut order = VecDeque::with_capacity(24);

    while Instant::now() < deadline {
        match consumer.recv_timeout(Duration::from_millis(250)) {
            Ok(ev) => {
                if order.len() == order.capacity() {
                    let _ = order.pop_front();
                }
                order.push_back(ev.kind);

                match ev.kind {
                    SCStreamOutputType::Audio => {
                        audio_seen += 1;
                        audio_pts.push(ev.pts_seconds);
                        if !audio_printed {
                            audio_printed = true;
                            println!(
                                "First system-audio buffer: pts={:.6}s dur={:.6}s samples={} rate={:?}Hz channels={:?} bits={:?} float={}",
                                ev.pts_seconds,
                                ev.duration_seconds,
                                ev.num_samples,
                                ev.sample_rate_hz,
                                ev.channels,
                                ev.bits_per_channel,
                                ev.is_float
                            );
                        }
                    }
                    SCStreamOutputType::Microphone => {
                        mic_seen += 1;
                        mic_pts.push(ev.pts_seconds);
                        if !mic_printed {
                            mic_printed = true;
                            println!(
                                "First microphone buffer: pts={:.6}s dur={:.6}s samples={} rate={:?}Hz channels={:?} bits={:?} float={}",
                                ev.pts_seconds,
                                ev.duration_seconds,
                                ev.num_samples,
                                ev.sample_rate_hz,
                                ev.channels,
                                ev.bits_per_channel,
                                ev.is_float
                            );
                        }
                    }
                    SCStreamOutputType::Screen => {}
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let transport_stats = consumer.stats_snapshot();
    let missing_format_description = callback_audit
        .missing_format_description
        .load(Ordering::Relaxed);

    stream
        .stop_capture()
        .context("failed to stop stream capture")?;

    println!();
    println!("Probe summary");
    println!("audio buffers: {audio_seen}");
    println!("microphone buffers: {mic_seen}");
    println!(
        "transport: capacity={}, high_water={}, in_flight={}, enqueued={}, dequeued={}, slot_miss_drops={}, fill_failures={}, queue_full_drops={}, recycle_failures={}",
        transport_stats.capacity,
        transport_stats.ready_depth_high_water,
        transport_stats.in_flight,
        transport_stats.enqueued,
        transport_stats.dequeued,
        transport_stats.slot_miss_drops,
        transport_stats.fill_failures,
        transport_stats.queue_full_drops,
        transport_stats.recycle_failures
    );
    println!(
        "callback_contract: missing_format_description={}",
        missing_format_description
    );

    if audio_seen == 0 || mic_seen == 0 {
        println!(
            "One stream is missing. Common causes: TCC denied, missing NSMicrophoneUsageDescription, or no active display."
        );
        return Ok(());
    }

    let paired = audio_pts.len().min(mic_pts.len());
    let index_aligned: Vec<f64> = (0..paired).map(|i| mic_pts[i] - audio_pts[i]).collect();
    let nearest = nearest_neighbor_deltas(&mic_pts, &audio_pts);

    println!("Recent callback order (oldest->newest): {:?}", order);
    summarize_deltas("Index-aligned delta (mic - system)", &index_aligned);
    summarize_deltas("Nearest-neighbor delta (mic - nearest system)", &nearest);
    println!("Interpretation: separate output callbacks were observed for system and microphone.");

    Ok(())
}
