use std::env;
use std::path::PathBuf;

use super::{
    AsrBackend, AsrProfile, ChannelMode, CliError, ParseOutcome, SpeakerLabels, TranscribeConfig,
    VadBackend,
};

#[allow(dead_code)]
pub(super) fn parse_args() -> Result<ParseOutcome, CliError> {
    parse_args_from(env::args().skip(1))
}

pub(super) fn parse_args_from(
    args: impl Iterator<Item = String>,
) -> Result<ParseOutcome, CliError> {
    let mut config = TranscribeConfig::default();
    let mut args = args;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(ParseOutcome::Help),
            "--duration-sec" => {
                config.duration_sec =
                    parse_u64(&read_value(&mut args, "--duration-sec")?, "--duration-sec")?;
            }
            "--input-wav" => {
                config.input_wav = PathBuf::from(read_value(&mut args, "--input-wav")?);
            }
            "--out-wav" => {
                config.out_wav = PathBuf::from(read_value(&mut args, "--out-wav")?);
            }
            "--out-jsonl" => {
                config.out_jsonl = PathBuf::from(read_value(&mut args, "--out-jsonl")?);
            }
            "--out-manifest" => {
                config.out_manifest = PathBuf::from(read_value(&mut args, "--out-manifest")?);
            }
            "--sample-rate" => {
                config.sample_rate_hz =
                    parse_u32(&read_value(&mut args, "--sample-rate")?, "--sample-rate")?;
            }
            "--asr-backend" => {
                config.asr_backend = AsrBackend::parse(&read_value(&mut args, "--asr-backend")?)?;
            }
            "--asr-model" => {
                config.asr_model = PathBuf::from(read_value(&mut args, "--asr-model")?);
            }
            "--asr-language" => {
                config.asr_language = read_value(&mut args, "--asr-language")?;
            }
            "--asr-threads" => {
                config.asr_threads =
                    parse_usize(&read_value(&mut args, "--asr-threads")?, "--asr-threads")?;
            }
            "--asr-profile" => {
                config.asr_profile = AsrProfile::parse(&read_value(&mut args, "--asr-profile")?)?;
            }
            "--vad-backend" => {
                config.vad_backend = VadBackend::parse(&read_value(&mut args, "--vad-backend")?)?;
            }
            "--vad-threshold" => {
                config.vad_threshold = parse_f32(
                    &read_value(&mut args, "--vad-threshold")?,
                    "--vad-threshold",
                )?;
            }
            "--vad-min-speech-ms" => {
                config.vad_min_speech_ms = parse_u32(
                    &read_value(&mut args, "--vad-min-speech-ms")?,
                    "--vad-min-speech-ms",
                )?;
            }
            "--vad-min-silence-ms" => {
                config.vad_min_silence_ms = parse_u32(
                    &read_value(&mut args, "--vad-min-silence-ms")?,
                    "--vad-min-silence-ms",
                )?;
            }
            "--llm-cleanup" => {
                config.llm_cleanup = true;
            }
            "--llm-endpoint" => {
                config.llm_endpoint = Some(read_value(&mut args, "--llm-endpoint")?);
            }
            "--llm-model" => {
                config.llm_model = Some(read_value(&mut args, "--llm-model")?);
            }
            "--llm-timeout-ms" => {
                config.llm_timeout_ms = parse_u64(
                    &read_value(&mut args, "--llm-timeout-ms")?,
                    "--llm-timeout-ms",
                )?;
            }
            "--llm-max-queue" => {
                config.llm_max_queue = parse_usize(
                    &read_value(&mut args, "--llm-max-queue")?,
                    "--llm-max-queue",
                )?;
            }
            "--llm-retries" => {
                config.llm_retries =
                    parse_usize(&read_value(&mut args, "--llm-retries")?, "--llm-retries")?;
            }
            "--live-chunked" => {
                config.live_chunked = true;
            }
            "--live-stream" => {
                config.live_stream = true;
            }
            "--chunk-window-ms" => {
                config.chunk_window_ms = parse_u64(
                    &read_value(&mut args, "--chunk-window-ms")?,
                    "--chunk-window-ms",
                )?;
            }
            "--chunk-stride-ms" => {
                config.chunk_stride_ms = parse_u64(
                    &read_value(&mut args, "--chunk-stride-ms")?,
                    "--chunk-stride-ms",
                )?;
            }
            "--chunk-queue-cap" => {
                config.chunk_queue_cap = parse_usize(
                    &read_value(&mut args, "--chunk-queue-cap")?,
                    "--chunk-queue-cap",
                )?;
            }
            "--live-asr-workers" => {
                config.live_asr_workers = parse_usize(
                    &read_value(&mut args, "--live-asr-workers")?,
                    "--live-asr-workers",
                )?;
            }
            "--keep-temp-audio" => {
                config.keep_temp_audio = true;
            }
            "--transcribe-channels" => {
                config.channel_mode =
                    ChannelMode::parse(&read_value(&mut args, "--transcribe-channels")?)?;
            }
            "--speaker-labels" => {
                config.speaker_labels =
                    SpeakerLabels::parse(&read_value(&mut args, "--speaker-labels")?)?;
            }
            "--benchmark-runs" => {
                config.benchmark_runs = parse_usize(
                    &read_value(&mut args, "--benchmark-runs")?,
                    "--benchmark-runs",
                )?;
            }
            "--model-doctor" => {
                config.model_doctor = true;
            }
            "--replay-jsonl" => {
                config.replay_jsonl = Some(PathBuf::from(read_value(&mut args, "--replay-jsonl")?));
            }
            "--preflight" => {
                config.preflight = true;
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::new(format!("unknown option `{arg}`")));
            }
            _ => {
                return Err(CliError::new(format!(
                    "unexpected positional argument `{arg}`; use named flags only"
                )));
            }
        }
    }

    config.validate()?;
    Ok(ParseOutcome::Config(config))
}

fn read_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, CliError> {
    args.next()
        .ok_or_else(|| CliError::new(format!("`{flag}` requires a value")))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, CliError> {
    value
        .parse::<u64>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_u32(value: &str, flag: &str) -> Result<u32, CliError> {
    value
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, CliError> {
    value
        .parse::<usize>()
        .map_err(|_| CliError::new(format!("`{flag}` expects an integer, got `{value}`")))
}

fn parse_f32(value: &str, flag: &str) -> Result<f32, CliError> {
    value.parse::<f32>().map_err(|_| {
        CliError::new(format!(
            "`{flag}` expects a floating-point value, got `{value}`"
        ))
    })
}
