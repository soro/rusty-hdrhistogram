#![cfg(feature = "cli")]

use hdrhistogram::encoding::{
    encode_double_histogram_log_line_with_max_value_unit_ratio, encode_histogram_log_line_with_max_value_unit_ratio,
    histogram_log_start_time_line,
};
use hdrhistogram::st::{DoubleHistogram, Histogram};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn run_with_stdin(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hdrhistogram"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn write_temp_input(test_name: &str, contents: &str) -> PathBuf {
    let path = temp_output_prefix(test_name);
    fs::write(&path, contents).unwrap();
    path
}

fn sample_log() -> String {
    let mut histogram = Histogram::builder()
        .highest_trackable_value(10_000)
        .significant_digits(2)
        .build()
        .unwrap();
    histogram.record_value_with_count(100, 3).unwrap();
    format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
    )
}

fn tagged_log() -> String {
    let mut untagged = Histogram::builder()
        .highest_trackable_value(10_000)
        .significant_digits(2)
        .build()
        .unwrap();
    untagged.record_value_with_count(100, 2).unwrap();
    let mut tagged = Histogram::builder()
        .highest_trackable_value(10_000)
        .significant_digits(2)
        .build()
        .unwrap();
    tagged.record_value_with_count(1_000, 7).unwrap();
    tagged.meta_data.set_tag_string("phase-a".to_string());
    format!(
        "{}{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&untagged, 10.0, 11.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&tagged, 11.0, 12.0, 1.0).unwrap(),
    )
}

fn double_log() -> String {
    let mut histogram: DoubleHistogram = DoubleHistogram::builder().significant_digits(3).build().unwrap();
    histogram.record_value_with_count(1.5, 2).unwrap();
    histogram.record_value(12.0).unwrap();
    format!(
        "{}{}",
        histogram_log_start_time_line(40.0),
        encode_double_histogram_log_line_with_max_value_unit_ratio(&histogram, 40.0, 41.0, 1.0).unwrap(),
    )
}

fn temp_output_prefix(test_name: &str) -> PathBuf {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("hdrhistogram-{}-{}-{}", test_name, std::process::id(), unique))
}

fn temp_output_pattern(test_name: &str) -> (PathBuf, String) {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();
    let pattern = std::env::temp_dir().join(format!("hdrhistogram-{}-%pid-%date-{}", test_name, unique));
    (pattern, unique)
}

fn find_replaced_output_prefix(test_name: &str, unique: &str) -> PathBuf {
    let prefix = format!("hdrhistogram-{}-", test_name);
    for entry in fs::read_dir(std::env::temp_dir()).unwrap() {
        let path = entry.unwrap().path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with(&prefix) && file_name.ends_with(unique) {
            return path;
        }
    }
    panic!("did not find output file for {test_name}");
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), suffix))
}

#[test]
fn tags_command_scans_without_decoding_payloads() {
    let output = run_with_stdin(
        &["log", "tags"],
        "#[StartTime: 10.000 (seconds since epoch)]\nTag=phase-a,10.000,1.000,42.000,not-base64\n",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!("phase-a\n", String::from_utf8(output.stdout).unwrap());
}

#[test]
fn filter_command_scans_without_decoding_payloads() {
    let output = run_with_stdin(
        &["log", "filter", "--tag", "phase-a"],
        "10.000,1.000,1.000,not-base64\nTag=phase-a,11.000,1.000,42.000,also-not-base64\n",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!("Tag=phase-a,11.000,1.000,42.000,also-not-base64\n", stdout);
}

#[test]
fn process_command_writes_percentile_distribution_to_stdout() {
    let log = sample_log();

    let output = run_with_stdin(&["log", "process", "--csv", "--output-value-unit-ratio", "1"], &log);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("#[StartTime: 10.000"));
    assert!(stdout.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(stdout.contains(",3,"));
}

#[test]
fn process_command_accepts_java_style_options_at_top_level() {
    let log = sample_log();

    let output = run_with_stdin(&["-csv", "-outputValueUnitRatio", "1"], &log);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("#[StartTime: 10.000"));
    assert!(stdout.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(stdout.contains(",3,"));
}

#[test]
fn java_style_moving_window_without_output_prefix_is_ignored_like_java() {
    let log = sample_log();

    let output = run_with_stdin(&["-csv", "-mwp", "90", "-mwpl", "1000", "-outputValueUnitRatio", "1"], &log);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(!stdout.contains("\"Window_Count\""));
}

#[test]
fn java_style_input_file_tag_range_and_ticks_options_are_supported() {
    let input = write_temp_input("input-tag-range", &tagged_log());

    let output = run_with_stdin(
        &[
            "-csv",
            "-i",
            input.to_str().unwrap(),
            "-tag",
            "phase-a",
            "-start",
            "1",
            "-end",
            "1",
            "-percentilesOutputTicksPerHalf",
            "2",
            "-outputValueUnitRatio",
            "1",
        ],
        "",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(stdout.contains(",7,"));

    fs::remove_file(input).unwrap();
}

#[test]
fn java_style_coordinated_omission_correction_option_is_supported() {
    let log = sample_log();

    let output = run_with_stdin(
        &[
            "-csv",
            "-outputValueUnitRatio",
            "1",
            "-correctLogWithKnownCoordinatedOmission",
            "50",
        ],
        &log,
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(",6,"));
}

#[test]
fn java_style_processes_double_histogram_logs() {
    let log = double_log();

    let output = run_with_stdin(&["-csv", "-outputValueUnitRatio", "1"], &log);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(stdout.contains(",3,"));
}

#[test]
fn java_style_verbose_flag_uses_debug_error_output() {
    let output = run_with_stdin(&["-v", "-csv"], "bad\n");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("InvalidLogLine(\"bad\")"));
}

#[test]
fn java_style_listtags_uses_java_output_shape_without_decoding_payloads() {
    let output = run_with_stdin(
        &["-listtags"],
        "10.000,1.000,1.000,not-base64\nTag=phase-a,11.000,1.000,42.000,also-not-base64\n",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!("Tags found in input file:\n[NO TAG (default)]\nphase-a\n", stdout);
}

#[test]
fn summary_command_can_emit_json_without_decoding_payloads() {
    let output = run_with_stdin(
        &["log", "summary", "--json"],
        "#[StartTime: 10.000 (seconds since epoch)]\n10.000,1.000,1.000,not-base64\nTag=phase-a,11.000,1.000,42.000,also-not-base64\n",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        "{\"total_intervals\":2,\"matched_intervals\":2,\"start_time_sec\":10.000,\"base_time_sec\":0.000,\"tags\":[null,\"phase-a\"]}\n",
        stdout
    );
}

#[test]
fn inspect_command_can_emit_filtered_json_without_decoding_payloads() {
    let output = run_with_stdin(
        &["log", "inspect", "--json", "--tag", "phase-a"],
        "#[StartTime: 10.000 (seconds since epoch)]\n10.000,1.000,1.000,not-base64\nTag=phase-a,11.000,1.000,42.000,also-not-base64\n",
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        "[{\"tag\":\"phase-a\",\"start_timestamp_sec\":11.000,\"interval_length_sec\":1.000,\"max_value\":42.000,\"absolute_start_time_sec\":11.000,\"absolute_end_time_sec\":12.000,\"relative_start_time_sec\":1.000,\"relative_end_time_sec\":2.000}]\n",
        stdout
    );
}

#[test]
fn java_style_output_prefix_writes_interval_hgrm_and_moving_window_files() {
    let log = sample_log();
    let output_prefix = temp_output_prefix("output-prefix");
    let hgrm_output = suffixed_path(&output_prefix, "hgrm");
    let moving_window_output = suffixed_path(&output_prefix, "mwp");
    let _ = fs::remove_file(&output_prefix);
    let _ = fs::remove_file(&hgrm_output);
    let _ = fs::remove_file(&moving_window_output);

    let output = run_with_stdin(
        &[
            "-csv",
            "-o",
            output_prefix.to_str().unwrap(),
            "-outputValueUnitRatio",
            "1",
            "-mwp",
            "90",
            "-mwpl",
            "1000",
        ],
        &log,
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty());
    let interval_log = fs::read_to_string(&output_prefix).unwrap();
    let percentile_log = fs::read_to_string(&hgrm_output).unwrap();
    let moving_window_log = fs::read_to_string(&moving_window_output).unwrap();
    assert!(interval_log
        .starts_with("#[Interval percentile log between 0.000 and <Infinite> seconds (relative to StartTime)]\n#[StartTime: 10.000"));
    assert!(interval_log.contains("\"Timestamp\",\"Int_Count\""));
    assert!(percentile_log.starts_with(
        "#[Overall percentile distribution between 0.000 and <Infinite> seconds (relative to StartTime)]\n#[StartTime: 10.000"
    ));
    assert!(percentile_log.contains("\"Value\",\"Percentile\",\"TotalCount\""));
    assert!(moving_window_log.starts_with(
        "#[Moving window log for 90 percentile between 0.000 and <Infinite> seconds (relative to StartTime)]\n\"Timestamp\",\"Window_Count\",\"90%'ile\""
    ));

    fs::remove_file(output_prefix).unwrap();
    fs::remove_file(hgrm_output).unwrap();
    fs::remove_file(moving_window_output).unwrap();
}

#[test]
fn java_style_output_prefix_replaces_pid_and_date_tokens() {
    let log = sample_log();
    let (output_pattern, unique) = temp_output_pattern("replacement");

    let output = run_with_stdin(
        &["-csv", "-o", output_pattern.to_str().unwrap(), "-outputValueUnitRatio", "1"],
        &log,
    );

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let output_prefix = find_replaced_output_prefix("replacement", &unique);
    let hgrm_output = suffixed_path(&output_prefix, "hgrm");
    assert!(!output_prefix.to_string_lossy().contains("%pid"));
    assert!(!output_prefix.to_string_lossy().contains("%date"));
    assert!(output_prefix.exists());
    assert!(hgrm_output.exists());

    fs::remove_file(output_prefix).unwrap();
    fs::remove_file(hgrm_output).unwrap();
}
