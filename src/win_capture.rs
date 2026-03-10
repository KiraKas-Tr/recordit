//! Windows audio capture backend using WASAPI (Windows Audio Session API).
//!
//! This module replaces ScreenCaptureKit on Windows with two separate WASAPI
//! capture loops:
//!   - **Microphone** — a standard capture-endpoint (input device).
//!   - **System audio** — a loopback capture from the default render-endpoint
//!     (records everything the speakers play, equivalent to macOS system-audio
//!     capture via ScreenCaptureKit).
//!
//! Both streams produce `f32` mono samples at the requested `target_rate_hz`.
//! If the device native rate differs we resample with the same linear
//! interpolation used on macOS (`resample_linear_mono`).
//!
//! # Parallelism
//!
//! Each stream runs in its own thread and forwards chunks through the same
//! `PreallocatedProducer` / `PreallocatedConsumer` ring used on macOS.  The
//! consumer side is identical to the macOS path.
//!
//! # Safety
//!
//! WASAPI is a COM-based API.  Every thread that calls COM must initialise
//! COM first.  We use `CoInitializeEx` with the `COINIT_MULTITHREADED` flag
//! and call `CoUninitialize` on thread exit (via a RAII guard).

#![cfg(target_os = "windows")]

use crate::capture_api::{CaptureChunk, CaptureStream};
use crate::rt_transport::{preallocated_spsc, PreallocatedConsumer, PreallocatedProducer};
use anyhow::{bail, Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::{
    Win32::Foundation::HANDLE,
    Win32::Media::Audio::{
        eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDevice,
        IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
        WAVEFORMATEXTENSIBLE,
    },
    Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    },
    Win32::System::Threading::{CreateEventW, WaitForSingleObject},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WIN_CAPTURE_RING_CAPACITY: usize = 512;
const WIN_MAX_MONO_SAMPLES_PER_CHUNK: usize = 16_384;
/// How long to wait for audio data from WASAPI before checking stop flag.
const WIN_WAIT_TIMEOUT_MS: u32 = 200;

// WAVE format tags — defined here because the windows crate exposes them as
// feature-gated items that may not always be available under the GNU target.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

// ---------------------------------------------------------------------------
// Public: stream kind tag (mirrors SCStreamOutputType on macOS)
// ---------------------------------------------------------------------------

/// Discriminator for audio chunks produced by Windows capture threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinStreamKind {
    Microphone,
    SystemAudio,
}

impl WinStreamKind {
    pub fn to_capture_stream(self) -> CaptureStream {
        match self {
            Self::Microphone => CaptureStream::Microphone,
            Self::SystemAudio => CaptureStream::SystemAudio,
        }
    }
}

// ---------------------------------------------------------------------------
// Reusable chunk slot (same as macOS ReusableTimedChunk pattern)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WinAudioChunk {
    pub kind: WinStreamKind,
    /// Presentation timestamp in seconds since capture start.
    pub pts_seconds: f64,
    pub sample_rate_hz: u32,
    /// Buffer of f32 mono samples (only first `valid_samples` are live).
    pub mono_samples: Vec<f32>,
    pub valid_samples: usize,
}

impl WinAudioChunk {
    pub fn with_capacity(max_samples: usize) -> Self {
        Self {
            kind: WinStreamKind::SystemAudio,
            pts_seconds: 0.0,
            sample_rate_hz: 0,
            mono_samples: vec![0.0_f32; max_samples],
            valid_samples: 0,
        }
    }

    pub fn mono_slice(&self) -> &[f32] {
        &self.mono_samples[..self.valid_samples]
    }

    pub fn to_capture_chunk(&self) -> CaptureChunk {
        CaptureChunk {
            stream: self.kind.to_capture_stream(),
            pts_seconds: self.pts_seconds,
            sample_rate_hz: self.sample_rate_hz,
            mono_samples: self.mono_slice().to_vec(),
        }
    }
}

// ---------------------------------------------------------------------------
// COM init guard
// ---------------------------------------------------------------------------

struct ComGuard;

impl ComGuard {
    fn init() -> Result<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // S_FALSE means COM was already initialised on this thread — that's OK.
        if hr.is_err() {
            let code = hr.0;
            // RPC_E_CHANGED_MODE (0x80010106) means already init'd with different model;
            // we log but proceed because the existing model usually works.
            if code == 0x80010106_u32 as i32 {
                eprintln!(
                    "[win_capture] COM already initialised with different threading model on this thread; proceeding."
                );
                return Ok(Self);
            }
            bail!("CoInitializeEx failed: HRESULT 0x{:08X}", hr.0);
        }
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

// ---------------------------------------------------------------------------
// Device helpers
// ---------------------------------------------------------------------------

fn create_device_enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .context("failed to create IMMDeviceEnumerator")
    }
}

fn get_default_microphone(enumerator: &IMMDeviceEnumerator) -> Result<IMMDevice> {
    unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .context("failed to get default microphone endpoint")
    }
}

fn get_default_render_device(enumerator: &IMMDeviceEnumerator) -> Result<IMMDevice> {
    unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .context("failed to get default render (speakers) endpoint")
    }
}

// ---------------------------------------------------------------------------
// Format utilities
// ---------------------------------------------------------------------------

/// Inspect a WAVEFORMATEX and return (channels, sample_rate_hz, is_float).
/// Returns Err if the format is too exotic to handle.
unsafe fn parse_wave_format(fmt: *const WAVEFORMATEX) -> Result<(u16, u32, bool)> {
    if fmt.is_null() {
        bail!("WAVEFORMATEX pointer is null");
    }
    // SAFETY: fmt is non-null and points to a valid WAVEFORMATEX (caller guarantee).
    let wfx = unsafe { &*fmt };
    let channels = wfx.nChannels;
    let sample_rate_hz = wfx.nSamplesPerSec;

    let is_float = if wfx.wFormatTag == WAVE_FORMAT_IEEE_FLOAT {
        true
    } else if wfx.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
        // WAVEFORMATEXTENSIBLE follows WAVEFORMATEX in memory.
        let wfex_ptr = fmt as *const WAVEFORMATEXTENSIBLE;
        // SAFETY: wfex_ptr is a valid WAVEFORMATEXTENSIBLE pointer cast from fmt.
        let wfex = unsafe { &*wfex_ptr };
        // SubFormat GUID for IEEE float: {00000003-0000-0010-8000-00aa00389b71}
        let float_subformat = windows::core::GUID::from_values(
            0x0000_0003,
            0x0000,
            0x0010,
            [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71],
        );
        // WAVEFORMATEXTENSIBLE is packed; copy SubFormat to avoid unaligned reference.
        // SAFETY: wfex_ptr was cast from a valid WAVEFORMATEXTENSIBLE pointer.
        let sub_format = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(wfex.SubFormat)) };
        sub_format == float_subformat
    } else {
        false
    };

    if channels == 0 {
        bail!("device reports 0 channels");
    }
    if sample_rate_hz == 0 {
        bail!("device reports 0 Hz sample rate");
    }
    Ok((channels, sample_rate_hz, is_float))
}

// ---------------------------------------------------------------------------
// Shared: one-shot WASAPI session setup
// ---------------------------------------------------------------------------

struct WasapiSession {
    audio_client: IAudioClient,
    capture_client: IAudioCaptureClient,
    event_handle: HANDLE,
    native_channels: u16,
    native_rate_hz: u32,
    native_is_float: bool,
    /// True when the session is for loopback (system audio) capture.
    /// Retained for diagnostics — checked during `open` to select stream flags.
    #[allow(dead_code)]
    is_loopback: bool,
}

impl WasapiSession {
    fn open(device: &IMMDevice, is_loopback: bool) -> Result<Self> {
        unsafe {
            let audio_client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .context("IMMDevice::Activate failed")?;

            // Query the mix format (what WASAPI prefers for shared mode).
            let mix_fmt_ptr = audio_client
                .GetMixFormat()
                .context("IAudioClient::GetMixFormat failed")?;
            let (native_channels, native_rate_hz, native_is_float) =
                parse_wave_format(mix_fmt_ptr)?;

            // Create an event so we can use WASAPI event-driven buffering.
            let event_handle =
                CreateEventW(None, false, false, None).context("CreateEventW failed")?;

            let stream_flags: u32 = if is_loopback {
                AUDCLNT_STREAMFLAGS_LOOPBACK
            } else {
                0u32
            };

            // 50 ms buffer (500_000 hns).
            let buffer_duration_hns: i64 = 500_000;

            audio_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    stream_flags | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    buffer_duration_hns,
                    0,
                    mix_fmt_ptr,
                    None,
                )
                .context("IAudioClient::Initialize failed")?;

            // Free the mix format — we've already captured what we need.
            CoTaskMemFree(Some(mix_fmt_ptr as *mut _));

            audio_client
                .SetEventHandle(event_handle)
                .context("IAudioClient::SetEventHandle failed")?;

            let capture_client: IAudioCaptureClient = audio_client
                .GetService()
                .context("IAudioClient::GetService (IAudioCaptureClient) failed")?;

            Ok(Self {
                audio_client,
                capture_client,
                event_handle,
                native_channels,
                native_rate_hz,
                native_is_float,
                is_loopback,
            })
        }
    }

    fn start(&self) -> Result<()> {
        unsafe {
            self.audio_client
                .Start()
                .context("IAudioClient::Start failed")
        }
    }

    fn stop(&self) {
        unsafe {
            let _ = self.audio_client.Stop();
        }
    }
}

// ---------------------------------------------------------------------------
// f32 mono conversion
// ---------------------------------------------------------------------------

/// Downmix any WASAPI buffer to f32 mono.
///
/// WASAPI shared mode always delivers IEEE float in its mix format, so we
/// handle that case directly.  If the device is reporting a different format
/// (rare) we fall back to a safe i16 → f32 normalisation.
unsafe fn extract_mono_f32(
    data_ptr: *const u8,
    num_frames: u32,
    channels: u16,
    is_float: bool,
    out_buf: &mut Vec<f32>,
) -> usize {
    let frames = num_frames as usize;
    let chs = channels as usize;
    out_buf.clear();

    if is_float {
        let float_ptr = data_ptr as *const f32;
        for frame in 0..frames {
            let mut sum = 0.0_f32;
            for ch in 0..chs {
                let idx = frame * chs + ch;
                // SAFETY: data_ptr points to a WASAPI buffer with num_frames * channels f32 values.
                sum += unsafe { *float_ptr.add(idx) };
            }
            out_buf.push(sum / chs as f32);
        }
    } else {
        // Interpret as i16 PCM (16-bit), normalise to [-1, 1].
        let i16_ptr = data_ptr as *const i16;
        let denom = i16::MAX as f32;
        for frame in 0..frames {
            let mut sum = 0.0_f32;
            for ch in 0..chs {
                let idx = frame * chs + ch;
                // SAFETY: data_ptr points to a WASAPI buffer with num_frames * channels i16 values.
                sum += unsafe { *i16_ptr.add(idx) } as f32 / denom;
            }
            out_buf.push(sum / chs as f32);
        }
    }

    out_buf.len()
}

// ---------------------------------------------------------------------------
// Capture loop (runs in its own thread)
// ---------------------------------------------------------------------------

/// Drain all available packets from the capture client and push chunks
/// through the preallocated ring.
///
/// Returns (frames_since_epoch, pts_seconds_of_last_chunk).
unsafe fn drain_capture_packets(
    session: &WasapiSession,
    producer: &PreallocatedProducer<WinAudioChunk>,
    kind: WinStreamKind,
    target_rate_hz: u32,
    pts_base_frames: &mut u64,
    pts_clock_rate: u32,
    scratch: &mut Vec<f32>,
) {
    loop {
        let mut data_ptr = std::ptr::null_mut::<u8>();
        let mut num_frames_available = 0u32;
        let mut flags = 0u32;
        let mut device_position = 0u64;
        let mut qpc_position = 0u64;

        // SAFETY: capture_client is valid; GetBuffer fills data_ptr with a WASAPI-owned buffer.
        let hr = unsafe {
            session.capture_client.GetBuffer(
                &mut data_ptr,
                &mut num_frames_available,
                &mut flags,
                Some(&mut device_position),
                Some(&mut qpc_position),
            )
        };

        if hr.is_err() || num_frames_available == 0 {
            break;
        }

        let pts_seconds = if pts_clock_rate > 0 {
            *pts_base_frames as f64 / pts_clock_rate as f64
        } else {
            0.0
        };
        *pts_base_frames += num_frames_available as u64;

        // Extract mono f32 samples.
        let native_rate = session.native_rate_hz;
        let native_channels = session.native_channels;
        let native_is_float = session.native_is_float;
        // SAFETY: data_ptr is valid until ReleaseBuffer; num_frames_available and channels match.
        unsafe {
            extract_mono_f32(
                data_ptr,
                num_frames_available,
                native_channels,
                native_is_float,
                scratch,
            )
        };

        // Resample if needed (target != native).
        let final_samples: Vec<f32> = if native_rate != target_rate_hz && native_rate > 0 {
            resample_linear_mono_win(scratch, native_rate, target_rate_hz)
        } else {
            scratch.clone()
        };

        // Release the WASAPI buffer before we do anything else.
        // SAFETY: num_frames_available was obtained from GetBuffer.
        let _ = unsafe { session.capture_client.ReleaseBuffer(num_frames_available) };

        // Push into the ring (non-blocking).
        producer.try_push_with(|slot| {
            slot.kind = kind;
            slot.pts_seconds = pts_seconds;
            slot.sample_rate_hz = target_rate_hz;
            let copy_len = final_samples.len().min(slot.mono_samples.len());
            slot.mono_samples[..copy_len].copy_from_slice(&final_samples[..copy_len]);
            slot.valid_samples = copy_len;
            true
        });
    }
}

/// Linear interpolation resampler (same algorithm as macOS path).
fn resample_linear_mono_win(samples: &[f32], input_rate_hz: u32, output_rate_hz: u32) -> Vec<f32> {
    if samples.is_empty()
        || input_rate_hz == output_rate_hz
        || input_rate_hz == 0
        || output_rate_hz == 0
    {
        return samples.to_vec();
    }

    let mut output_len = ((samples.len() as u128 * output_rate_hz as u128
        + input_rate_hz as u128 / 2)
        / input_rate_hz as u128) as usize;
    output_len = output_len.max(1);

    if samples.len() == 1 {
        return vec![samples[0]; output_len];
    }

    let mut output = Vec::with_capacity(output_len);
    let ratio = f64::from(input_rate_hz) / f64::from(output_rate_hz);
    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos.floor() as usize;
        if src_idx >= samples.len() - 1 {
            output.push(*samples.last().unwrap_or(&0.0));
            continue;
        }
        let frac = (src_pos - src_idx as f64) as f32;
        let a = samples[src_idx];
        let b = samples[src_idx + 1];
        output.push(a + ((b - a) * frac));
    }
    output
}

// ---------------------------------------------------------------------------
// Public: run both capture loops and forward to the shared consumer
// ---------------------------------------------------------------------------

/// Holds the two thread handles for the capture loops.
pub struct WinCaptureHandles {
    pub mic_thread: std::thread::JoinHandle<Result<()>>,
    pub sys_thread: std::thread::JoinHandle<Result<()>>,
}

/// Start Windows WASAPI capture for microphone + system audio loopback.
///
/// Returns `(consumer, handles)`.
/// The caller drives the same receive loop as macOS.
pub fn start_win_capture(
    target_rate_hz: u32,
    stop_flag: Arc<AtomicBool>,
) -> Result<(PreallocatedConsumer<WinAudioChunk>, WinCaptureHandles)> {
    let slots: Vec<WinAudioChunk> = (0..WIN_CAPTURE_RING_CAPACITY)
        .map(|_| WinAudioChunk::with_capacity(WIN_MAX_MONO_SAMPLES_PER_CHUNK))
        .collect();
    let (producer, consumer) = preallocated_spsc(slots);

    let mic_producer = producer.clone();
    let sys_producer = producer;
    let mic_stop = Arc::clone(&stop_flag);
    let sys_stop = Arc::clone(&stop_flag);

    // ── Microphone thread ──────────────────────────────────────────────────
    let mic_thread = std::thread::Builder::new()
        .name("win-capture-mic".to_string())
        .spawn(move || -> Result<()> {
            let _com = ComGuard::init()?;
            let enumerator = create_device_enumerator()?;
            let mic_device = get_default_microphone(&enumerator)?;
            let session = WasapiSession::open(&mic_device, false)?;
            session.start()?;

            let mut scratch = Vec::with_capacity(WIN_MAX_MONO_SAMPLES_PER_CHUNK * 2);
            let mut pts_base_frames: u64 = 0;
            let pts_clock_rate = session.native_rate_hz;

            loop {
                if mic_stop.load(Ordering::Relaxed) {
                    break;
                }
                unsafe {
                    let wait_result =
                        WaitForSingleObject(session.event_handle, WIN_WAIT_TIMEOUT_MS);
                    if wait_result.0 != 0 /* WAIT_OBJECT_0 is 0 */ && wait_result.0 != 0x102
                    /* WAIT_TIMEOUT */
                    {
                        break;
                    }
                    drain_capture_packets(
                        &session,
                        &mic_producer,
                        WinStreamKind::Microphone,
                        target_rate_hz,
                        &mut pts_base_frames,
                        pts_clock_rate,
                        &mut scratch,
                    );
                }
                if mic_stop.load(Ordering::Relaxed) {
                    break;
                }
            }

            session.stop();
            Ok(())
        })
        .context("failed to spawn mic capture thread")?;

    // ── System audio (loopback) thread ────────────────────────────────────
    let sys_thread = std::thread::Builder::new()
        .name("win-capture-sys".to_string())
        .spawn(move || -> Result<()> {
            let _com = ComGuard::init()?;
            let enumerator = create_device_enumerator()?;
            let render_device = get_default_render_device(&enumerator)?;
            let session = WasapiSession::open(&render_device, true)?;
            session.start()?;

            let mut scratch = Vec::with_capacity(WIN_MAX_MONO_SAMPLES_PER_CHUNK * 2);
            let mut pts_base_frames: u64 = 0;
            let pts_clock_rate = session.native_rate_hz;

            loop {
                if sys_stop.load(Ordering::Relaxed) {
                    break;
                }
                unsafe {
                    let wait_result =
                        WaitForSingleObject(session.event_handle, WIN_WAIT_TIMEOUT_MS);
                    if wait_result.0 != 0 && wait_result.0 != 0x102 {
                        break;
                    }
                    drain_capture_packets(
                        &session,
                        &sys_producer,
                        WinStreamKind::SystemAudio,
                        target_rate_hz,
                        &mut pts_base_frames,
                        pts_clock_rate,
                        &mut scratch,
                    );
                }
                if sys_stop.load(Ordering::Relaxed) {
                    break;
                }
            }

            session.stop();
            Ok(())
        })
        .context("failed to spawn system-audio capture thread")?;

    Ok((
        consumer,
        WinCaptureHandles {
            mic_thread,
            sys_thread,
        },
    ))
}

// ---------------------------------------------------------------------------
// Preflight helpers (used from preflight.rs on Windows)
// ---------------------------------------------------------------------------

/// Quick check: can we enumerate the default devices?
pub fn check_audio_capture_access() -> Result<(String, String)> {
    let _com = ComGuard::init()?;
    let enumerator = create_device_enumerator()?;

    // Try to open the default mic endpoint.
    let mic_device = get_default_microphone(&enumerator)
        .context("cannot open default microphone — check Privacy > Microphone settings")?;
    let sys_device = get_default_render_device(&enumerator)
        .context("cannot open default render device — no audio output device found")?;

    // Retrieve friendly names.
    let mic_name = device_friendly_name(&mic_device).unwrap_or_else(|_| "microphone".to_string());
    let sys_name =
        device_friendly_name(&sys_device).unwrap_or_else(|_| "render device".to_string());

    Ok((mic_name, sys_name))
}

fn device_friendly_name(device: &IMMDevice) -> Result<String> {
    use windows::Win32::System::Com::STGM_READ;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, PROPERTYKEY};

    // PKEY_Device_FriendlyName — same GUID/pid as DEVPKEY_Device_FriendlyName but
    // typed as PROPERTYKEY so it matches IPropertyStore::GetValue's signature.
    // {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
    let pkey_friendly_name = PROPERTYKEY {
        fmtid: windows::core::GUID::from_values(
            0xa45c_254e,
            0xdf1c,
            0x4efd,
            [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
        ),
        pid: 14,
    };

    unsafe {
        let store: IPropertyStore = device
            .OpenPropertyStore(STGM_READ)
            .context("failed to open property store")?;
        let prop = store
            .GetValue(&pkey_friendly_name as *const PROPERTYKEY)
            .context("failed to query FriendlyName")?;
        // Extract the string from the PROPVARIANT.
        let name = prop.to_string();
        Ok(name)
    }
}
