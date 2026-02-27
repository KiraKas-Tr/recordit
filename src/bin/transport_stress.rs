use recordit::rt_transport::{TransportStatsSnapshot, preallocated_spsc};
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct StressSlot {
    seq: u64,
    len: usize,
    payload: Vec<u8>,
}

impl StressSlot {
    fn with_capacity(payload_bytes: usize) -> Self {
        Self {
            seq: 0,
            len: 0,
            payload: vec![0; payload_bytes],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Config {
    iterations: u64,
    capacity: usize,
    payload_bytes: usize,
    consumer_delay_micros: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            iterations: 200_000,
            capacity: 256,
            payload_bytes: 4_096,
            consumer_delay_micros: 0,
        }
    }
}

fn parse_args() -> Result<Config, String> {
    let mut config = Config::default();
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iterations" => {
                let value = args.next().ok_or("missing value for --iterations")?;
                config.iterations = value
                    .parse()
                    .map_err(|_| format!("invalid --iterations value: {value}"))?;
            }
            "--capacity" => {
                let value = args.next().ok_or("missing value for --capacity")?;
                config.capacity = value
                    .parse()
                    .map_err(|_| format!("invalid --capacity value: {value}"))?;
            }
            "--payload-bytes" => {
                let value = args.next().ok_or("missing value for --payload-bytes")?;
                config.payload_bytes = value
                    .parse()
                    .map_err(|_| format!("invalid --payload-bytes value: {value}"))?;
            }
            "--consumer-delay-micros" => {
                let value = args
                    .next()
                    .ok_or("missing value for --consumer-delay-micros")?;
                config.consumer_delay_micros = value
                    .parse()
                    .map_err(|_| format!("invalid --consumer-delay-micros value: {value}"))?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if config.capacity == 0 {
        return Err("--capacity must be > 0".to_string());
    }
    if config.payload_bytes == 0 {
        return Err("--payload-bytes must be > 0".to_string());
    }

    Ok(config)
}

fn print_help() {
    println!("transport_stress options:");
    println!("  --iterations <n>             Number of producer attempts (default 200000)");
    println!("  --capacity <n>               Preallocated slot count (default 256)");
    println!("  --payload-bytes <n>          Bytes per slot payload (default 4096)");
    println!("  --consumer-delay-micros <n>  Artificial consumer delay (default 0)");
}

fn main() {
    let config = match parse_args() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("argument error: {err}");
            print_help();
            std::process::exit(2);
        }
    };

    let slots = (0..config.capacity)
        .map(|_| StressSlot::with_capacity(config.payload_bytes))
        .collect();
    let (producer, consumer) = preallocated_spsc(slots);

    let producer_done = Arc::new(AtomicBool::new(false));
    let consumed = Arc::new(AtomicU64::new(0));
    let ordering_errors = Arc::new(AtomicU64::new(0));

    let consumer_done = Arc::clone(&producer_done);
    let consumed_count = Arc::clone(&consumed);
    let ordering_error_count = Arc::clone(&ordering_errors);
    let consumer_delay = config.consumer_delay_micros;

    let consumer_thread = thread::spawn(move || {
        let mut last_seq = None;

        loop {
            match consumer.recv_timeout(Duration::from_millis(10)) {
                Ok(slot) => {
                    if let Some(previous) = last_seq {
                        if slot.seq <= previous {
                            ordering_error_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    last_seq = Some(slot.seq);
                    consumed_count.fetch_add(1, Ordering::Relaxed);

                    if consumer_delay > 0 {
                        thread::sleep(Duration::from_micros(consumer_delay));
                    }

                    consumer.recycle(slot);
                }
                Err(_) if consumer_done.load(Ordering::Relaxed) => break consumer.stats_snapshot(),
                Err(_) => {}
            }
        }
    });

    let start = Instant::now();
    for seq in 1..=config.iterations {
        producer.try_push_with(|slot| {
            slot.seq = seq;
            slot.len = slot.payload.len();
            for (idx, byte) in slot.payload.iter_mut().enumerate() {
                *byte = ((seq as usize + idx) & 0xFF) as u8;
            }
            true
        });
    }
    let producer_elapsed = start.elapsed();
    producer_done.store(true, Ordering::Relaxed);

    let stats = consumer_thread.join().expect("consumer thread panicked");
    let consumed = consumed.load(Ordering::Relaxed);
    let ordering_errors = ordering_errors.load(Ordering::Relaxed);

    print_report(config, producer_elapsed, stats, consumed, ordering_errors);
}

fn print_report(
    config: Config,
    producer_elapsed: Duration,
    stats: TransportStatsSnapshot,
    consumed: u64,
    ordering_errors: u64,
) {
    let attempted = config.iterations;
    let elapsed_secs = producer_elapsed.as_secs_f64();
    let throughput = if elapsed_secs > 0.0 {
        attempted as f64 / elapsed_secs
    } else {
        0.0
    };

    println!("transport_stress");
    println!("iterations={attempted}");
    println!("capacity={}", config.capacity);
    println!("payload_bytes={}", config.payload_bytes);
    println!("consumer_delay_micros={}", config.consumer_delay_micros);
    println!(
        "producer_elapsed_ms={:.3}",
        producer_elapsed.as_secs_f64() * 1_000.0
    );
    println!("producer_attempts_per_sec={throughput:.0}");
    println!("transport_capacity={}", stats.capacity);
    println!(
        "transport_ready_depth_high_water={}",
        stats.ready_depth_high_water
    );
    println!("transport_in_flight={}", stats.in_flight);
    println!("enqueued={}", stats.enqueued);
    println!("dequeued={}", stats.dequeued);
    println!("slot_miss_drops={}", stats.slot_miss_drops);
    println!("fill_failures={}", stats.fill_failures);
    println!("queue_full_drops={}", stats.queue_full_drops);
    println!("recycle_failures={}", stats.recycle_failures);
    println!("consumed={consumed}");
    println!("ordering_errors={ordering_errors}");
}
