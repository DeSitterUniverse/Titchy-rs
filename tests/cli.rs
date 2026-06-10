use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn cli_compress_decompress_and_get_sample() {
    let dir = temp_case_dir("titchy_cli");
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("samples.txt");
    let compressed = dir.join("samples.tchy");
    let output = dir.join("decoded.txt");
    fs::write(&input, "1 2 3 4 5 6 7 8 9").unwrap();

    let bin = env!("CARGO_BIN_EXE_titchy");
    let compress = Command::new(bin)
        .args([
            "compress",
            "--bits",
            "8",
            "--samples-per-chunk",
            "3",
            "--chunks-per-split",
            "2",
            input.to_str().unwrap(),
            compressed.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compress.status.success(),
        "{}",
        String::from_utf8_lossy(&compress.stderr)
    );

    let decompress = Command::new(bin)
        .args([
            "decompress",
            compressed.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        decompress.status.success(),
        "{}",
        String::from_utf8_lossy(&decompress.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n"
    );

    let get = Command::new(bin)
        .args(["get", compressed.to_str().unwrap(), "6"])
        .output()
        .unwrap();
    assert!(
        get.status.success(),
        "{}",
        String::from_utf8_lossy(&get.stderr)
    );
    assert_eq!(String::from_utf8(get.stdout).unwrap(), "7\n");
}

#[test]
fn cli_bench_reports_reproducible_metrics() {
    let dir = temp_case_dir("titchy_bench");
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("samples.txt");
    fs::write(&input, "42 43 42 43 42 43 42 43 42 43 42 43").unwrap();

    let bin = env!("CARGO_BIN_EXE_titchy");
    let bench = Command::new(bin)
        .args([
            "bench",
            "--bits",
            "8",
            "--samples-per-chunk",
            "2",
            "--chunks-per-split",
            "3",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        bench.status.success(),
        "{}",
        String::from_utf8_lossy(&bench.stderr)
    );
    let stdout = String::from_utf8(bench.stdout).unwrap();
    assert!(stdout.contains("samples=12"));
    assert!(stdout.contains("compressed_bits="));
    assert!(stdout.contains("compression_ratio="));
    assert!(stdout.contains("encode_ns="));
    assert!(stdout.contains("decode_ns="));
}

#[test]
fn cli_stream_archive_round_trips_samples() {
    let dir = temp_case_dir("titchy_stream");
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("samples.txt");
    let stream = dir.join("samples.tstr");
    let output = dir.join("decoded.txt");
    fs::write(&input, "1 2 3 4 5 6 7 8 9 10 11").unwrap();
    let bin = env!("CARGO_BIN_EXE_titchy");

    let encode = Command::new(bin)
        .args([
            "stream-encode",
            "--bits",
            "8",
            "--samples-per-chunk",
            "2",
            "--chunks-per-split",
            "3",
            "--chunks-per-packet",
            "2",
            input.to_str().unwrap(),
            stream.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        encode.status.success(),
        "{}",
        String::from_utf8_lossy(&encode.stderr)
    );

    let decode = Command::new(bin)
        .args([
            "stream-decode",
            stream.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        decode.status.success(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n"
    );
}

#[test]
fn cli_access_cost_and_quick_sweep_report_paper_metrics() {
    let dir = temp_case_dir("titchy_metrics");
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("samples.txt");
    let compressed = dir.join("samples.tchy");
    fs::write(&input, "1 2 3 4 1 2 3 4 1 2 3 4").unwrap();
    let bin = env!("CARGO_BIN_EXE_titchy");

    let status = Command::new(bin)
        .args([
            "compress",
            "--bits",
            "8",
            input.to_str().unwrap(),
            compressed.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let access = Command::new(bin)
        .args(["access-cost", compressed.to_str().unwrap(), "0", "32"])
        .output()
        .unwrap();
    assert!(access.status.success());
    let access_stdout = String::from_utf8(access.stdout).unwrap();
    assert!(access_stdout.contains("best_case_bits="));
    assert!(access_stdout.contains("worst_case_bits="));

    let sweep = Command::new(bin)
        .args(["sweep", "--quick", "--bits", "8", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        sweep.status.success(),
        "{}",
        String::from_utf8_lossy(&sweep.stderr)
    );
    let sweep_stdout = String::from_utf8(sweep.stdout).unwrap();
    assert!(sweep_stdout.starts_with("samples_per_chunk,chunks_per_split"));
    assert!(sweep_stdout.lines().count() > 2);
}

#[test]
fn cli_raw_binary_round_trip_honors_endianness() {
    let dir = temp_case_dir("titchy_raw");
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("samples.bin");
    let compressed = dir.join("samples.tchy");
    let output = dir.join("decoded.bin");
    let raw = [0x12, 0x34, 0xab, 0xcd, 0x01, 0x02];
    fs::write(&input, raw).unwrap();
    let bin = env!("CARGO_BIN_EXE_titchy");

    let encode = Command::new(bin)
        .args([
            "compress-raw",
            "--bits",
            "16",
            "--endian",
            "big",
            input.to_str().unwrap(),
            compressed.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        encode.status.success(),
        "{}",
        String::from_utf8_lossy(&encode.stderr)
    );

    let decode = Command::new(bin)
        .args([
            "decompress-raw",
            compressed.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        decode.status.success(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    assert_eq!(fs::read(output).unwrap(), raw);
}

#[test]
fn cli_demo_reports_compression_verification_and_random_access() {
    let bin = env!("CARGO_BIN_EXE_titchy");
    let demo = Command::new(bin)
        .args(["demo", "--samples", "4096"])
        .output()
        .unwrap();

    assert!(
        demo.status.success(),
        "{}",
        String::from_utf8_lossy(&demo.stderr)
    );
    let stdout = String::from_utf8(demo.stdout).unwrap();
    assert!(stdout.contains("Titchy deterministic sensor demo"));
    assert!(stdout.contains("samples:             4096"));
    assert!(stdout.contains("lossless verified:   yes"));
    assert!(stdout.contains("serialized ratio:"));
    assert!(stdout.contains("encode throughput:"));
    assert!(stdout.contains("decode throughput:"));
    assert!(stdout.contains("sample access:"));
    assert!(stdout.contains("range access:"));
    assert!(stdout.contains("bit access:"));
    assert!(stdout.contains("memory-limited:"));
}

#[test]
fn cli_benchmark_writes_a_reproducible_markdown_report() {
    let dir = temp_case_dir("titchy_benchmark");
    fs::create_dir_all(&dir).unwrap();
    let report = dir.join("benchmarks.md");
    let bin = env!("CARGO_BIN_EXE_titchy");
    let benchmark = Command::new(bin)
        .args([
            "benchmark",
            "--samples",
            "4096",
            "--iterations",
            "2",
            "--access-operations",
            "128",
            "--markdown",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        benchmark.status.success(),
        "{}",
        String::from_utf8_lossy(&benchmark.stderr)
    );
    let stdout = String::from_utf8(benchmark.stdout).unwrap();
    let markdown = fs::read_to_string(report).unwrap();
    for expected in [
        "# Titchy Benchmark Results",
        "unlimited",
        "1 KiB",
        "Serialized ratio",
        "Encode MB/s",
        "Decode MB/s",
        "Sample access ns",
        "Range access ns",
    ] {
        assert!(stdout.contains(expected), "stdout missing {expected}");
        assert!(markdown.contains(expected), "report missing {expected}");
    }
}

fn temp_case_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}_{unique}"))
}
