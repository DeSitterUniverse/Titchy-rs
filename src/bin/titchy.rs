use std::{
    env,
    fs::{self, File},
    hint::black_box,
    io::Cursor,
    process::ExitCode,
    time::Instant,
};

use titchy_rs::{
    compress_raw_bytes, compress_samples, decompress_raw_bytes, decompress_samples,
    get_by_bit_index, get_by_sample_index, get_range_by_sample_index, synthetic_sensor_samples,
    titchy_access_bounds, CompressedTitchy, IndexedTitchyReader, SampleEndian, StreamDecoder,
    StreamEncoder, StreamPacket, TitchyConfig,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(usage());
    }
    let command = args.remove(0);
    match command.as_str() {
        "compress" => compress_command(&args),
        "compress-raw" => compress_raw_command(&args),
        "decompress" => decompress_command(&args),
        "decompress-raw" => decompress_raw_command(&args),
        "get" => get_command(&args),
        "range" => range_command(&args),
        "bench" => bench_command(&args),
        "stream-encode" => stream_encode_command(&args),
        "stream-decode" => stream_decode_command(&args),
        "access-cost" => access_cost_command(&args),
        "sweep" => sweep_command(&args),
        "demo" => demo_command(&args),
        "benchmark" => benchmark_command(&args),
        _ => Err(usage()),
    }
}

#[derive(Debug)]
struct BenchmarkResult {
    mode: &'static str,
    paper_ratio: f64,
    serialized_ratio: f64,
    encode_mb_s: f64,
    decode_mb_s: f64,
    sample_access_ns: f64,
    range_access_ns: f64,
}

fn benchmark_command(args: &[String]) -> Result<(), String> {
    let mut sample_count = 250_000usize;
    let mut iterations = 5usize;
    let mut access_operations = 10_000usize;
    let mut markdown_path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--samples" => {
                i += 1;
                sample_count = parse_arg(args, i, "--samples")?;
            }
            "--iterations" => {
                i += 1;
                iterations = parse_arg(args, i, "--iterations")?;
            }
            "--access-operations" => {
                i += 1;
                access_operations = parse_arg(args, i, "--access-operations")?;
            }
            "--markdown" => {
                i += 1;
                markdown_path = Some(
                    args.get(i)
                        .ok_or_else(|| "--markdown requires a path".to_string())?
                        .clone(),
                );
            }
            _ => return Err(usage()),
        }
        i += 1;
    }
    if sample_count < 64 || iterations == 0 || access_operations == 0 {
        return Err(
            "benchmark requires at least 64 samples, one iteration, and one access operation"
                .to_string(),
        );
    }

    let samples = synthetic_sensor_samples(sample_count);
    let results = [
        benchmark_mode(&samples, None, iterations, access_operations, "unlimited")?,
        benchmark_mode(&samples, Some(1024), iterations, access_operations, "1 KiB")?,
    ];
    let report = benchmark_markdown(sample_count, iterations, access_operations, &results);
    print!("{report}");
    if let Some(path) = markdown_path {
        fs::write(path, &report).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn benchmark_mode(
    samples: &[u64],
    dictionary_cap: Option<usize>,
    iterations: usize,
    access_operations: usize,
    mode: &'static str,
) -> Result<BenchmarkResult, String> {
    let config = TitchyConfig::new(16, 4, 1000)
        .map_err(|err| err.to_string())?
        .with_max_active_dictionary_bytes(dictionary_cap);
    let uncompressed_bytes = samples.len() * 2;
    let mut encode_times = Vec::with_capacity(iterations);
    let mut decode_times = Vec::with_capacity(iterations);
    let mut latest = None;

    for _ in 0..iterations {
        let encode_start = Instant::now();
        let compressed =
            compress_samples(black_box(samples), config.clone()).map_err(|err| err.to_string())?;
        encode_times.push(encode_start.elapsed());

        let decode_start = Instant::now();
        let decoded = decompress_samples(black_box(&compressed)).map_err(|err| err.to_string())?;
        decode_times.push(decode_start.elapsed());
        if decoded != samples {
            return Err("benchmark decode verification failed".to_string());
        }
        latest = Some(compressed);
    }

    let compressed = latest.expect("benchmark runs at least once");
    let serialized = compressed.to_bytes().map_err(|err| err.to_string())?;
    let sample_access_start = Instant::now();
    let mut sample_reader = IndexedTitchyReader::open(Cursor::new(serialized.clone()))
        .map_err(|err| err.to_string())?;
    let mut checksum = 0u64;
    for operation in 0..access_operations {
        let index = operation.wrapping_mul(104_729) % samples.len();
        checksum ^= sample_reader
            .get_by_sample_index(index)
            .map_err(|err| err.to_string())?;
    }
    black_box(checksum);
    let sample_access_ns =
        sample_access_start.elapsed().as_nanos() as f64 / access_operations as f64;

    let range_access_start = Instant::now();
    let mut range_reader = IndexedTitchyReader::open(Cursor::new(serialized.clone()))
        .map_err(|err| err.to_string())?;
    let range_len = 16usize;
    let range_positions = samples.len() - range_len + 1;
    let mut range_checksum = 0u64;
    for operation in 0..access_operations {
        let start = operation.wrapping_mul(104_729) % range_positions;
        let range = range_reader
            .get_range_by_sample_index(start, range_len)
            .map_err(|err| err.to_string())?;
        range_checksum ^= range[0];
    }
    black_box(range_checksum);
    let range_access_ns = range_access_start.elapsed().as_nanos() as f64 / access_operations as f64;

    let uncompressed_bits = uncompressed_bytes * 8;
    Ok(BenchmarkResult {
        mode,
        paper_ratio: compressed.paper_compressed_bit_len() as f64 / uncompressed_bits as f64,
        serialized_ratio: serialized.len() as f64 / uncompressed_bytes as f64,
        encode_mb_s: throughput_mb_s(uncompressed_bytes, median_duration(&mut encode_times)),
        decode_mb_s: throughput_mb_s(uncompressed_bytes, median_duration(&mut decode_times)),
        sample_access_ns,
        range_access_ns,
    })
}

fn median_duration(durations: &mut [std::time::Duration]) -> std::time::Duration {
    durations.sort_unstable();
    durations[durations.len() / 2]
}

fn benchmark_markdown(
    sample_count: usize,
    iterations: usize,
    access_operations: usize,
    results: &[BenchmarkResult],
) -> String {
    let mut report = format!(
        "# Titchy Benchmark Results\n\n\
         Deterministic 16-bit sensor data, {sample_count} samples, {iterations} timing iterations, \
         and {access_operations} indexed access operations per latency measurement.\n\n\
         Command: `cargo run --release -- benchmark --samples {sample_count} --iterations {iterations} \
         --access-operations {access_operations}`\n\n\
         | Dictionary | Paper ratio | Serialized ratio | Encode MB/s | Decode MB/s | Sample access ns | Range access ns |\n\
         | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n"
    );
    for result in results {
        report.push_str(&format!(
            "| {} | {:.4} | {:.4} | {:.2} | {:.2} | {:.0} | {:.0} |\n",
            result.mode,
            result.paper_ratio,
            result.serialized_ratio,
            result.encode_mb_s,
            result.decode_mb_s,
            result.sample_access_ns,
            result.range_access_ns
        ));
    }
    report.push_str(
        "\nRatios are compressed size divided by the original fixed-width sample size; lower is better. \
         Range latency retrieves 16 samples. Timing results are machine-dependent.\n",
    );
    report
}

fn demo_command(args: &[String]) -> Result<(), String> {
    let mut sample_count = 100_000usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--samples" => {
                i += 1;
                sample_count = parse_arg(args, i, "--samples")?;
            }
            _ => return Err(usage()),
        }
        i += 1;
    }
    if sample_count < 64 {
        return Err("--samples must be at least 64".to_string());
    }

    let samples = synthetic_sensor_samples(sample_count);
    let config = TitchyConfig::new(16, 4, 1000).map_err(|err| err.to_string())?;
    let uncompressed_bytes = samples.len() * 2;

    let encode_start = Instant::now();
    let compressed = compress_samples(&samples, config.clone()).map_err(|err| err.to_string())?;
    let encode_elapsed = encode_start.elapsed();
    let serialized = compressed.to_bytes().map_err(|err| err.to_string())?;

    let decode_start = Instant::now();
    let decoded = decompress_samples(&compressed).map_err(|err| err.to_string())?;
    let decode_elapsed = decode_start.elapsed();
    let verified = decoded == samples;
    if !verified {
        return Err("demo decode verification failed".to_string());
    }

    let sample_index = sample_count / 2;
    let range_start = sample_index.saturating_sub(4);
    let range =
        get_range_by_sample_index(&compressed, range_start, 8).map_err(|err| err.to_string())?;
    let sample = get_by_sample_index(&compressed, sample_index).map_err(|err| err.to_string())?;
    let bit_index = sample_index * 16 + 15;
    let bit = get_by_bit_index(&compressed, bit_index).map_err(|err| err.to_string())?;

    let limited = compress_samples(
        &samples,
        config.with_max_active_dictionary_bytes(Some(1024)),
    )
    .map_err(|err| err.to_string())?;
    let limited_ratio = limited
        .serialized_bit_len()
        .map_err(|err| err.to_string())? as f64
        / (uncompressed_bytes * 8) as f64;

    println!("Titchy deterministic sensor demo");
    println!("--------------------------------");
    println!("samples:             {sample_count}");
    println!("sample width:        16 bits");
    println!("uncompressed:        {uncompressed_bytes} bytes");
    println!("compressed:          {} bytes", serialized.len());
    println!(
        "serialized ratio:    {:.4} ({:.1}% smaller)",
        serialized.len() as f64 / uncompressed_bytes as f64,
        (1.0 - serialized.len() as f64 / uncompressed_bytes as f64) * 100.0
    );
    println!(
        "encode throughput:   {:.2} MB/s",
        throughput_mb_s(uncompressed_bytes, encode_elapsed)
    );
    println!(
        "decode throughput:   {:.2} MB/s",
        throughput_mb_s(uncompressed_bytes, decode_elapsed)
    );
    println!("lossless verified:   yes");
    println!("sample access:       [{sample_index}] = {sample}");
    println!(
        "range access:        [{range_start}..{}] = {range:?}",
        range_start + 8
    );
    println!("bit access:          [{bit_index}] = {bit}");
    println!("memory-limited:      1024-byte dictionary, ratio {limited_ratio:.4}");
    Ok(())
}

fn throughput_mb_s(bytes: usize, elapsed: std::time::Duration) -> f64 {
    bytes as f64 / elapsed.as_secs_f64().max(1e-9) / 1_000_000.0
}

fn compress_command(args: &[String]) -> Result<(), String> {
    let (config, positional) = parse_compress_options(args)?;
    if positional.len() != 2 {
        return Err(usage());
    }
    let samples = read_samples(&positional[0])?;
    let compressed = compress_samples(&samples, config).map_err(|err| err.to_string())?;
    fs::write(
        &positional[1],
        compressed.to_bytes().map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn compress_raw_command(args: &[String]) -> Result<(), String> {
    let (config, positional) = parse_compress_options(args)?;
    if positional.len() != 2 {
        return Err(usage());
    }
    let raw = fs::read(&positional[0]).map_err(|err| err.to_string())?;
    let compressed = compress_raw_bytes(&raw, config).map_err(|err| err.to_string())?;
    fs::write(
        &positional[1],
        compressed.to_bytes().map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn parse_compress_options(args: &[String]) -> Result<(TitchyConfig, Vec<String>), String> {
    let mut bits = None;
    let mut samples_per_chunk = 4u8;
    let mut chunks_per_split = 1000u16;
    let mut max_active_dictionary_bytes = None;
    let mut endian = SampleEndian::Little;
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bits" => {
                i += 1;
                bits = Some(parse_arg(args, i, "--bits")?);
            }
            "--samples-per-chunk" => {
                i += 1;
                samples_per_chunk = parse_arg(args, i, "--samples-per-chunk")?;
            }
            "--chunks-per-split" => {
                i += 1;
                chunks_per_split = parse_arg(args, i, "--chunks-per-split")?;
            }
            "--max-active-dictionary-bytes" => {
                i += 1;
                max_active_dictionary_bytes =
                    Some(parse_arg(args, i, "--max-active-dictionary-bytes")?);
            }
            "--endian" => {
                i += 1;
                endian = match args.get(i).map(String::as_str) {
                    Some("little") => SampleEndian::Little,
                    Some("big") => SampleEndian::Big,
                    _ => return Err("--endian must be little or big".to_string()),
                };
            }
            value => positional.push(value.to_owned()),
        }
        i += 1;
    }
    let bits = bits.ok_or_else(|| "--bits is required for compression".to_string())?;
    let config = TitchyConfig::new(bits, samples_per_chunk, chunks_per_split)
        .map_err(|err| err.to_string())?
        .with_endian(endian)
        .with_max_active_dictionary_bytes(max_active_dictionary_bytes);
    Ok((config, positional))
}

fn decompress_command(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(usage());
    }
    let bytes = fs::read(&args[0]).map_err(|err| err.to_string())?;
    let compressed = CompressedTitchy::from_bytes(&bytes).map_err(|err| err.to_string())?;
    let samples = decompress_samples(&compressed).map_err(|err| err.to_string())?;
    write_samples(&args[1], &samples)
}

fn decompress_raw_command(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(usage());
    }
    let bytes = fs::read(&args[0]).map_err(|err| err.to_string())?;
    let compressed = CompressedTitchy::from_bytes(&bytes).map_err(|err| err.to_string())?;
    let raw = decompress_raw_bytes(&compressed).map_err(|err| err.to_string())?;
    fs::write(&args[1], raw).map_err(|err| err.to_string())
}

fn get_command(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(usage());
    }
    let index = args[1].parse::<usize>().map_err(|err| err.to_string())?;
    let file = File::open(&args[0]).map_err(|err| err.to_string())?;
    let sample = IndexedTitchyReader::open(file)
        .map_err(|err| err.to_string())?
        .get_by_sample_index(index)
        .map_err(|err| err.to_string())?;
    println!("{sample}");
    Ok(())
}

fn range_command(args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err(usage());
    }
    let start = args[1].parse::<usize>().map_err(|err| err.to_string())?;
    let len = args[2].parse::<usize>().map_err(|err| err.to_string())?;
    let file = File::open(&args[0]).map_err(|err| err.to_string())?;
    let samples = IndexedTitchyReader::open(file)
        .map_err(|err| err.to_string())?
        .get_range_by_sample_index(start, len)
        .map_err(|err| err.to_string())?;
    for sample in samples {
        println!("{sample}");
    }
    Ok(())
}

fn bench_command(args: &[String]) -> Result<(), String> {
    let (config, positional) = parse_compress_options(args)?;
    if positional.len() != 1 {
        return Err(usage());
    }
    let samples = read_samples(&positional[0])?;
    let uncompressed_bits = samples.len() * config.bits_per_sample as usize;

    let encode_start = Instant::now();
    let compressed = compress_samples(&samples, config).map_err(|err| err.to_string())?;
    let encode_ns = encode_start.elapsed().as_nanos();

    let decode_start = Instant::now();
    let decoded = decompress_samples(&compressed).map_err(|err| err.to_string())?;
    let decode_ns = decode_start.elapsed().as_nanos();
    if decoded != samples {
        return Err("decode verification failed during benchmark".to_string());
    }

    let compressed_bits = compressed.compressed_bit_len();
    let ratio = if uncompressed_bits == 0 {
        0.0
    } else {
        compressed_bits as f64 / uncompressed_bits as f64
    };
    println!("samples={}", samples.len());
    println!("uncompressed_bits={uncompressed_bits}");
    println!("compressed_bits={compressed_bits}");
    println!("compression_ratio={ratio:.6}");
    println!("encode_ns={encode_ns}");
    println!("decode_ns={decode_ns}");
    Ok(())
}

fn stream_encode_command(args: &[String]) -> Result<(), String> {
    let mut chunks_per_packet = 1usize;
    let mut forwarded = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--chunks-per-packet" {
            i += 1;
            chunks_per_packet = parse_arg(args, i, "--chunks-per-packet")?;
        } else {
            forwarded.push(args[i].clone());
        }
        i += 1;
    }
    let (config, positional) = parse_compress_options(&forwarded)?;
    if positional.len() != 2 {
        return Err(usage());
    }
    let samples = read_samples(&positional[0])?;
    let mut encoder =
        StreamEncoder::new(config.clone(), chunks_per_packet).map_err(|err| err.to_string())?;
    let mut packets = Vec::new();
    for &sample in &samples {
        if let Some(packet) = encoder.push_sample(sample).map_err(|err| err.to_string())? {
            packets.push(packet);
        }
    }
    packets.extend(encoder.finish().map_err(|err| err.to_string())?);

    let mut archive = Vec::new();
    archive.extend_from_slice(b"TSTR");
    archive.push(1);
    archive.push(config.bits_per_sample);
    archive.push(config.samples_per_chunk);
    archive.extend_from_slice(&config.chunks_per_split.to_le_bytes());
    archive.extend_from_slice(&(samples.len() as u64).to_le_bytes());
    archive.extend_from_slice(&(packets.len() as u32).to_le_bytes());
    for packet in packets {
        if packet.chunk_count() > u16::MAX as usize {
            return Err("stream archive packet contains too many chunks".to_string());
        }
        archive.extend_from_slice(&(packet.chunk_count() as u16).to_le_bytes());
        archive.extend_from_slice(&(packet.bytes().len() as u32).to_le_bytes());
        archive.extend_from_slice(packet.bytes());
    }
    fs::write(&positional[1], archive).map_err(|err| err.to_string())
}

fn stream_decode_command(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(usage());
    }
    let bytes = fs::read(&args[0]).map_err(|err| err.to_string())?;
    let mut cursor = SliceCursor::new(&bytes);
    if cursor.read_exact(4)? != b"TSTR" || cursor.read_u8()? != 1 {
        return Err("invalid TSTR stream archive".to_string());
    }
    let bits = cursor.read_u8()?;
    let samples_per_chunk = cursor.read_u8()?;
    let chunks_per_split = cursor.read_u16()?;
    let original_sample_count = cursor.read_u64()? as usize;
    let packet_count = cursor.read_u32()? as usize;
    let config = TitchyConfig::new(bits, samples_per_chunk, chunks_per_split)
        .map_err(|err| err.to_string())?;
    let mut decoder = StreamDecoder::new(config).map_err(|err| err.to_string())?;
    let mut samples = Vec::new();
    for _ in 0..packet_count {
        let chunk_count = cursor.read_u16()? as usize;
        let packet_len = cursor.read_u32()? as usize;
        let packet = StreamPacket::from_bytes(cursor.read_exact(packet_len)?.to_vec(), chunk_count)
            .map_err(|err| err.to_string())?;
        samples.extend(
            decoder
                .decode_packet(&packet)
                .map_err(|err| err.to_string())?,
        );
    }
    samples.truncate(original_sample_count);
    write_samples(&args[1], &samples)
}

fn access_cost_command(args: &[String]) -> Result<(), String> {
    if args.len() != 3 {
        return Err(usage());
    }
    let bytes = fs::read(&args[0]).map_err(|err| err.to_string())?;
    let compressed = CompressedTitchy::from_bytes(&bytes).map_err(|err| err.to_string())?;
    let start_bit = args[1].parse::<usize>().map_err(|err| err.to_string())?;
    let requested_bits = args[2].parse::<usize>().map_err(|err| err.to_string())?;
    let bounds = titchy_access_bounds(&compressed, start_bit, requested_bits)
        .map_err(|err| err.to_string())?;
    println!("chunk_count={}", bounds.chunk_count);
    println!("split_count={}", bounds.split_count);
    println!("best_case_bits={}", bounds.best_case_bits);
    println!("worst_case_bits={}", bounds.worst_case_bits);
    Ok(())
}

fn sweep_command(args: &[String]) -> Result<(), String> {
    let mut bits = None;
    let mut quick = false;
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bits" => {
                i += 1;
                bits = Some(parse_arg(args, i, "--bits")?);
            }
            "--quick" => quick = true,
            value => positional.push(value.to_owned()),
        }
        i += 1;
    }
    if positional.len() != 1 {
        return Err(usage());
    }
    let bits = bits.ok_or_else(|| "--bits is required for sweep".to_string())?;
    TitchyConfig::new(bits, 1, 100).map_err(|err| err.to_string())?;
    let samples = read_samples(&positional[0])?;
    let maximum_c = (510usize / bits as usize).min(16) as u8;
    let c_values: Vec<u8> = if quick {
        [1, 4]
            .into_iter()
            .filter(|value| *value <= maximum_c)
            .collect()
    } else {
        (1..=maximum_c).collect()
    };
    let k_values: &[u16] = if quick {
        &[100, 1000]
    } else {
        &[100, 300, 1000, 3000]
    };
    let caps: &[Option<usize>] = if quick {
        &[None, Some(1024)]
    } else {
        &[None, Some(1024), Some(10 * 1024)]
    };
    println!(
        "samples_per_chunk,chunks_per_split,dictionary_cap_bytes,paper_ratio,serialized_ratio,encode_mb_s,decode_mb_s,dictionary_bases"
    );
    let uncompressed_bits = samples.len() * bits as usize;
    let uncompressed_bytes = uncompressed_bits as f64 / 8.0;
    for &c in &c_values {
        for &k in k_values {
            for &cap in caps {
                let config = TitchyConfig::new(bits, c, k)
                    .map_err(|err| err.to_string())?
                    .with_max_active_dictionary_bytes(cap);
                let encode_start = Instant::now();
                let compressed =
                    compress_samples(&samples, config).map_err(|err| err.to_string())?;
                let encode_seconds = encode_start.elapsed().as_secs_f64().max(1e-9);
                let decode_start = Instant::now();
                let decoded = decompress_samples(&compressed).map_err(|err| err.to_string())?;
                let decode_seconds = decode_start.elapsed().as_secs_f64().max(1e-9);
                if decoded != samples {
                    return Err("decode verification failed during sweep".to_string());
                }
                let paper_ratio = if uncompressed_bits == 0 {
                    0.0
                } else {
                    compressed.paper_compressed_bit_len() as f64 / uncompressed_bits as f64
                };
                let serialized_ratio = if uncompressed_bits == 0 {
                    0.0
                } else {
                    compressed
                        .serialized_bit_len()
                        .map_err(|err| err.to_string())? as f64
                        / uncompressed_bits as f64
                };
                println!(
                    "{c},{k},{},{paper_ratio:.6},{serialized_ratio:.6},{:.3},{:.3},{}",
                    cap.map_or_else(|| "unlimited".to_string(), |value| value.to_string()),
                    uncompressed_bytes / encode_seconds / 1_000_000.0,
                    uncompressed_bytes / decode_seconds / 1_000_000.0,
                    compressed.dictionary_len()
                );
            }
        }
    }
    Ok(())
}

fn write_samples(path: &str, samples: &[u64]) -> Result<(), String> {
    let mut out = String::new();
    for sample in samples {
        out.push_str(&sample.to_string());
        out.push('\n');
    }
    fs::write(path, out).map_err(|err| err.to_string())
}

fn read_samples(path: &str) -> Result<Vec<u64>, String> {
    let input = fs::read_to_string(path).map_err(|err| err.to_string())?;
    input
        .split_whitespace()
        .map(|token| token.parse::<u64>().map_err(|err| err.to_string()))
        .collect::<Result<Vec<_>, _>>()
}

fn parse_arg<T>(args: &[String], index: usize, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    args.get(index)
        .ok_or_else(|| format!("{name} requires a value"))?
        .parse::<T>()
        .map_err(|err| err.to_string())
}

struct SliceCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.pos + len > self.bytes.len() {
            return Err("truncated stream archive".to_string());
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.bytes[start..self.pos])
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.read_exact(2)?.try_into().unwrap()))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.read_exact(4)?.try_into().unwrap()))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.read_exact(8)?.try_into().unwrap()))
    }
}

fn usage() -> String {
    "usage:
  titchy compress --bits N [--samples-per-chunk C] [--chunks-per-split K] [--max-active-dictionary-bytes BYTES] <input.txt> <output.tchy>
  titchy compress-raw --bits N [--endian little|big] [compression options] <input.bin> <output.tchy>
  titchy decompress <input.tchy> <output.txt>
  titchy decompress-raw <input.tchy> <output.bin>
  titchy get <input.tchy> <sample-index>
  titchy range <input.tchy> <start> <len>
  titchy bench --bits N [--samples-per-chunk C] [--chunks-per-split K] [--max-active-dictionary-bytes BYTES] <input.txt>
  titchy stream-encode --bits N [--samples-per-chunk C] [--chunks-per-split K] [--chunks-per-packet N] <input.txt> <output.tstr>
  titchy stream-decode <input.tstr> <output.txt>
  titchy access-cost <input.tchy> <start-bit> <bit-length>
  titchy sweep [--quick] --bits N <input.txt>
  titchy demo [--samples N]
  titchy benchmark [--samples N] [--iterations N] [--access-operations N] [--markdown PATH]"
        .to_string()
}
