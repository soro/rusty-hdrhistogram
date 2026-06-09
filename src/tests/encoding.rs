use crate::concurrent::{DoubleRecorder, ResizableConcurrentHistogram};
use crate::core::histogram_settings::{HistogramSettings, V2_ENCODING_HEADER_SIZE, V2_ENCODING_MAX_WORD_SIZE_IN_BYTES};
use crate::encoding::*;
use crate::st::{DoubleHistogram, Histogram};
#[cfg(feature = "encoding-base64")]
use std::io::Cursor;

#[test]
fn histogram_settings_estimate_v2_encoding_capacity() {
    let settings = HistogramSettings::new(1, 1_024, 2).unwrap();
    assert_eq!(
        V2_ENCODING_HEADER_SIZE + settings.counts_array_length as usize * V2_ENCODING_MAX_WORD_SIZE_IN_BYTES,
        settings.v2_encoding_capacity()
    );

    let relevant_length = settings.counts_array_index(123).saturating_add(1);
    assert_eq!(
        V2_ENCODING_HEADER_SIZE + relevant_length as usize * V2_ENCODING_MAX_WORD_SIZE_IN_BYTES,
        settings.v2_encoding_capacity_for_value(123)
    );
}

#[test]
fn histogram_v2_roundtrip_preserves_counts() {
    let mut histogram = Histogram::with_high_sigvdig(3_600_000_000, 3).unwrap();
    histogram.record_value_with_count(0, 3).unwrap();
    histogram.record_value(1).unwrap();
    histogram.record_value_with_count(10_000, 2).unwrap();
    histogram.record_value_with_count(123_456_789, 4).unwrap();

    let encoded = encode_histogram_v2(&histogram).unwrap();
    assert_eq!(&encoded[..4], &V2_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_histogram_v2(&encoded).unwrap();
    assert!(histogram.equals(&decoded));
    assert_eq!(histogram.get_total_count(), decoded.get_total_count());
    assert_eq!(histogram.get_max_value(), decoded.get_max_value());
    assert_eq!(histogram.get_count_at_value(0), decoded.get_count_at_value(0));
    assert_eq!(histogram.get_count_at_value(10_000), decoded.get_count_at_value(10_000));
    assert_eq!(histogram.get_count_at_value(123_456_789), decoded.get_count_at_value(123_456_789));
}

#[test]
fn histogram_v2_decode_canonicalizes_normalizing_index_offset() {
    let mut histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let mut encoded = encode_histogram_v2(&histogram).unwrap();
    encoded[8..12].copy_from_slice(&i32::MAX.to_be_bytes());

    let decoded = decode_histogram_v2(&encoded).unwrap();
    assert_eq!(3, decoded.get_total_count());
    assert_eq!(Some(3), decoded.get_count_at_value(100));
}

#[test]
fn generic_decode_detects_integer_histogram() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value_with_count(2_000, 7).unwrap();

    let encoded = encode_histogram_v2(&histogram).unwrap();
    match decode(&encoded).unwrap() {
        DecodedHistogram::Integer(decoded) => assert_eq!(Some(7), decoded.get_count_at_value(2_000)),
        DecodedHistogram::Double(_) => panic!("decoded integer histogram as double"),
    }
}

#[test]
fn concurrent_histogram_v2_encoding_uses_read_view() {
    let histogram = ResizableConcurrentHistogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value_with_count(2_000, 7).unwrap();

    let view = histogram.read_view();
    let encoded = encode_histogram_v2(&view).unwrap();
    let decoded = decode_histogram_v2(&encoded).unwrap();

    assert_eq!(Some(7), decoded.get_count_at_value(2_000));
}

#[test]
fn double_histogram_v2_roundtrip_preserves_counts() {
    let mut histogram = DoubleHistogram::new(3).unwrap();
    histogram.record_value_with_count(1.5, 2).unwrap();
    histogram.record_value(12.0).unwrap();
    histogram.record_value_with_count(128.0, 3).unwrap();

    let encoded = encode_double_histogram_v2(&histogram).unwrap();
    assert_eq!(&encoded[..4], &DOUBLE_HISTOGRAM_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_double_histogram_v2(&encoded).unwrap();
    assert_eq!(histogram.get_total_count(), decoded.get_total_count());
    assert_eq!(histogram.get_count_at_value(1.5), decoded.get_count_at_value(1.5));
    assert_eq!(histogram.get_count_at_value(12.0), decoded.get_count_at_value(12.0));
    assert_eq!(histogram.get_count_at_value(128.0), decoded.get_count_at_value(128.0));
    assert!(histogram.values_are_equivalent(histogram.get_value_at_percentile(100.0), decoded.get_value_at_percentile(100.0)));
}

#[test]
fn concurrent_double_snapshot_v2_roundtrip_preserves_counts() {
    let recorder = DoubleRecorder::builder()
        .highest_to_lowest_value_ratio(1024)
        .significant_digits(2)
        .build()
        .unwrap();
    recorder.record_value_with_count(1.5, 2).unwrap();
    recorder.record_value(12.0).unwrap();

    let sample = recorder.begin_interval_sample();
    let snapshot = sample.snapshot();
    let encoded = encode_concurrent_double_snapshot_v2(&snapshot).unwrap();
    assert_eq!(&encoded[..4], &DOUBLE_HISTOGRAM_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_double_histogram_v2(&encoded).unwrap();
    assert_eq!(snapshot.get_total_count(), decoded.get_total_count());
    assert_eq!(snapshot.get_count_at_value(1.5), decoded.get_count_at_value(1.5));
    assert_eq!(snapshot.get_count_at_value(12.0), decoded.get_count_at_value(12.0));
}

#[test]
fn decode_rejects_invalid_cookie() {
    assert!(matches!(
        decode_histogram(&0x12345678_u32.to_be_bytes()),
        Err(DecodeError::InvalidCookie(0x12345678))
    ));
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_histogram_roundtrip_preserves_counts() {
    let mut histogram = Histogram::with_high_sigvdig(1_000_000, 3).unwrap();
    histogram.record_value_with_count(42, 5).unwrap();
    histogram.record_value_with_count(999_999, 2).unwrap();

    let encoded = encode_histogram_compressed(&histogram).unwrap();
    assert_eq!(&encoded[..4], &V2_COMPRESSED_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_histogram_compressed(&encoded).unwrap();
    assert!(histogram.equals(&decoded));
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_double_histogram_roundtrip_preserves_counts() {
    let mut histogram = DoubleHistogram::new(3).unwrap();
    histogram.record_value_with_count(2.0, 3).unwrap();
    histogram.record_value_with_count(16.0, 4).unwrap();

    let encoded = encode_double_histogram_compressed(&histogram).unwrap();
    assert_eq!(&encoded[..4], &DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_double_histogram_compressed(&encoded).unwrap();
    assert_eq!(histogram.get_total_count(), decoded.get_total_count());
    assert_eq!(histogram.get_count_at_value(2.0), decoded.get_count_at_value(2.0));
    assert_eq!(histogram.get_count_at_value(16.0), decoded.get_count_at_value(16.0));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn base64_histogram_roundtrip_preserves_counts() {
    let mut histogram = Histogram::with_high_sigvdig(1_000, 2).unwrap();
    histogram.record_value_with_count(100, 9).unwrap();

    let encoded = encode_histogram_base64(&histogram).unwrap();
    let decoded = decode_histogram_base64(&encoded).unwrap();
    assert_eq!(Some(9), decoded.get_count_at_value(100));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_line_roundtrip_preserves_metadata_and_counts() {
    let mut histogram = Histogram::with_high_sigvdig(1_000, 2).unwrap();
    histogram.record_value_with_count(250, 6).unwrap();
    histogram.meta_data.set_tag_string("phase-a".to_string());

    let line = encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 12.5, 1.0).unwrap();
    match decode_histogram_log_line(&line).unwrap() {
        Some(HistogramLogRecord::Interval(entry)) => {
            assert_eq!(Some("phase-a".to_string()), entry.tag);
            assert_eq!(10.0, entry.start_timestamp_sec);
            assert_eq!(2.5, entry.interval_length_sec);
            match entry.histogram {
                DecodedHistogram::Integer(decoded) => assert_eq!(Some(6), decoded.get_count_at_value(250)),
                DecodedHistogram::Double(_) => panic!("decoded integer histogram log line as double"),
            }
        }
        _ => panic!("did not decode interval log line"),
    }
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_comment_lines_parse() {
    assert!(matches!(
        decode_histogram_log_line("#[StartTime: 10.125 (seconds since epoch)]").unwrap(),
        Some(HistogramLogRecord::StartTime(10.125))
    ));
    let start_time_line = histogram_log_start_time_line(10.125);
    assert!(start_time_line.starts_with("#[StartTime: 10.125 (seconds since epoch), "));
    assert!(start_time_line.ends_with("]\n"));
    assert!(matches!(
        decode_histogram_log_line(&start_time_line).unwrap(),
        Some(HistogramLogRecord::StartTime(10.125))
    ));
    assert!(matches!(
        decode_histogram_log_line("#[BaseTime: 9.000 (seconds since epoch)]").unwrap(),
        Some(HistogramLogRecord::BaseTime(9.0))
    ));
    assert!(matches!(
        decode_histogram_log_line(histogram_log_legend_line()).unwrap(),
        Some(HistogramLogRecord::Legend)
    ));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_scanner_defers_payload_decoding() {
    let line = "Tag=phase-a 10.000 1.000 42.000 not-base64";
    match scan_histogram_log_line(line).unwrap() {
        Some(HistogramLogScannedRecord::Interval(interval)) => {
            assert_eq!(Some("phase-a".to_string()), interval.tag);
            assert_eq!(10.0, interval.start_timestamp_sec);
            assert_eq!(1.0, interval.interval_length_sec);
            assert_eq!(42.0, interval.max_value);
            assert_eq!("not-base64", interval.compressed_histogram_base64);
            assert!(matches!(interval.decode_histogram(), Err(DecodeError::Base64(_))));
        }
        _ => panic!("did not scan interval log line"),
    }
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_scanner_reports_java_timing_semantics_without_decoding() {
    let log = format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        "Tag=phase-a,10.000,1.500,42.000,not-base64\n"
    );
    let mut scanner = HistogramLogScanner::new(Cursor::new(log));
    let interval = scanner.next_interval().unwrap().unwrap();

    assert_eq!(10.0, scanner.start_time_sec());
    assert_eq!(0.0, scanner.base_time_sec());
    assert_eq!(10.0, interval.absolute_start_time_sec);
    assert_eq!(11.5, interval.absolute_end_time_sec);
    assert_eq!(0.0, interval.relative_start_time_sec);
    assert_eq!(1.5, interval.relative_end_time_sec);
    assert!(matches!(interval.decode_histogram(), Err(DecodeError::Base64(_))));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_generates_interval_and_percentile_outputs() {
    let mut first = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    first.record_value_with_count(100, 3).unwrap();
    first.record_value(500).unwrap();

    let mut second = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    second.record_value_with_count(1_000, 2).unwrap();

    let log = format!(
        "{}{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&first, 10.0, 11.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&second, 11.0, 12.5, 1.0).unwrap(),
    );

    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };
    let report = generate_histogram_log_report(log.lines(), &config).unwrap();

    assert_eq!(10.0, report.start_time_sec);
    assert_eq!(2, report.processed_interval_count);
    assert!(report.interval_log.starts_with("\"Timestamp\",\"Int_Count\""));
    assert!(report.interval_log.contains("1.000,4,"));
    assert!(report.interval_log.contains("2.500,2,"));
    assert!(report.percentile_distribution.starts_with("\"Value\",\"Percentile\""));
    assert_eq!(None, report.moving_window_log);
}

#[cfg(feature = "encoding-base64")]
#[test]
fn double_histogram_log_report_generates_interval_and_percentile_outputs() {
    let mut histogram = DoubleHistogram::new(3).unwrap();
    histogram.record_value_with_count(1.5, 2).unwrap();
    histogram.record_value(12.0).unwrap();

    let log = format!(
        "{}{}",
        histogram_log_start_time_line(40.0),
        encode_double_histogram_log_line_with_max_value_unit_ratio(&histogram, 40.0, 41.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    assert_eq!(1, report.processed_interval_count);
    assert!(report.interval_log.starts_with("\"Timestamp\",\"Int_Count\""));
    assert!(report.interval_log.contains("1.000,3,"));
    assert!(report.percentile_distribution.starts_with("\"Value\",\"Percentile\""));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn double_histogram_log_line_roundtrip_preserves_metadata_and_counts() {
    let mut histogram = DoubleHistogram::new(3).unwrap();
    histogram.record_value_with_count(1.5, 2).unwrap();
    histogram.meta_data_mut().set_tag_string("phase-a".to_string());

    let line = encode_double_histogram_log_line_with_max_value_unit_ratio(&histogram, 40.0, 41.5, 1.0).unwrap();
    match decode_histogram_log_line(&line).unwrap() {
        Some(HistogramLogRecord::Interval(entry)) => {
            assert_eq!(Some("phase-a".to_string()), entry.tag);
            assert_eq!(40.0, entry.start_timestamp_sec);
            assert_eq!(1.5, entry.interval_length_sec);
            match entry.histogram {
                DecodedHistogram::Double(decoded) => assert_eq!(histogram.get_total_count(), decoded.get_total_count()),
                DecodedHistogram::Integer(_) => panic!("decoded double histogram log line as integer"),
            }
        }
        _ => panic!("did not decode interval log line"),
    }
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_filters_by_tag() {
    let mut untagged = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    untagged.record_value(100).unwrap();

    let mut tagged = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    tagged.record_value_with_count(1_000, 7).unwrap();
    tagged.meta_data.set_tag_string("phase-a".to_string());

    let log = format!(
        "{}{}{}",
        histogram_log_start_time_line(20.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&untagged, 20.0, 21.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&tagged, 21.0, 22.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        tag_filter: HistogramLogTagFilter::Tag("phase-a".to_string()),
        output_value_unit_ratio: 1.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    assert_eq!(1, report.processed_interval_count);
    assert_eq!(vec![None, Some("phase-a".to_string())], report.tags);
    assert!(report.interval_log.contains("2.000,7,"));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_can_emit_moving_window_output() {
    let mut first = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    first.record_value_with_count(100, 3).unwrap();

    let mut second = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    second.record_value_with_count(1_000, 2).unwrap();

    let log = format!(
        "{}{}{}",
        histogram_log_start_time_line(30.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&first, 30.0, 31.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&second, 31.0, 32.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: true,
        moving_window: Some(HistogramLogMovingWindowConfig {
            percentile_to_report: 90.0,
            length_sec: 0.5,
        }),
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    let moving_window_log = report.moving_window_log.unwrap();
    assert!(moving_window_log.starts_with("\"Timestamp\",\"Window_Count\",\"90%'ile\""));
    assert!(moving_window_log.contains("1.000,3,"));
    assert!(moving_window_log.contains("2.000,2,"));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_reader_writer_preserve_base_time_semantics() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let mut output = Vec::new();
    {
        let mut writer = HistogramLogWriter::with_max_value_unit_ratio(&mut output, 1.0).unwrap();
        writer.write_start_time(100.0).unwrap();
        writer.write_base_time(100.0).unwrap();
        writer.write_legend().unwrap();
        writer.write_interval(&histogram, 101.0, 102.0).unwrap();
        writer.flush().unwrap();
    }

    let log = String::from_utf8(output).unwrap();
    assert!(log.contains("#[BaseTime: 100.000"));
    assert!(log.contains("1.000,1.000,"));

    let mut reader = HistogramLogReader::new(Cursor::new(log));
    let interval = reader.next_interval().unwrap().unwrap();
    assert_eq!(100.0, reader.start_time_sec());
    assert_eq!(100.0, reader.base_time_sec());
    assert_eq!(1.0, interval.start_timestamp_sec);
    assert_eq!(101.0, interval.absolute_start_time_sec);
    assert_eq!(1.0, interval.relative_start_time_sec);
    assert_eq!(2.0, interval.relative_end_time_sec);
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_can_stream_input_and_outputs() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let log = format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: true,
        moving_window: Some(HistogramLogMovingWindowConfig {
            percentile_to_report: 99.0,
            length_sec: 60.0,
        }),
        ..HistogramLogReportConfig::default()
    };

    let mut interval_log = Vec::new();
    let mut percentile_distribution = Vec::new();
    let mut moving_window_log = Vec::new();
    let summary = write_histogram_log_report(
        Cursor::new(log),
        &config,
        &mut interval_log,
        &mut percentile_distribution,
        Some(&mut moving_window_log),
    )
    .unwrap();

    assert_eq!(1, summary.processed_interval_count);
    assert!(String::from_utf8(interval_log).unwrap().contains("1.000,3,"));
    assert!(String::from_utf8(percentile_distribution)
        .unwrap()
        .starts_with("\"Value\",\"Percentile\""));
    assert!(String::from_utf8(moving_window_log).unwrap().contains("\"99%'ile\""));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_applies_range_boundaries_to_interval_start() {
    let mut first = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    first.record_value(100).unwrap();
    let mut second = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    second.record_value_with_count(200, 2).unwrap();

    let log = format!(
        "{}{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&first, 10.0, 11.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&second, 11.0, 12.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        range_start_time_sec: 1.0,
        range_end_time_sec: 1.0,
        output_value_unit_ratio: 1.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    assert_eq!(1, report.processed_interval_count);
    assert!(report.interval_log.contains("2.000,2,"));
    assert!(!report.interval_log.contains("1.000,1,"));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_rejects_invalid_config() {
    let mut config = HistogramLogReportConfig {
        range_start_time_sec: 2.0,
        range_end_time_sec: 1.0,
        ..HistogramLogReportConfig::default()
    };
    assert!(matches!(
        generate_histogram_log_report([""].iter().copied(), &config),
        Err(DecodeError::InvalidLogLine(_))
    ));

    config = HistogramLogReportConfig {
        output_value_unit_ratio: 0.0,
        ..HistogramLogReportConfig::default()
    };
    assert!(matches!(
        generate_histogram_log_report([""].iter().copied(), &config),
        Err(DecodeError::InvalidLogLine(_))
    ));

    config = HistogramLogReportConfig {
        percentile_ticks_per_half_distance: 0,
        ..HistogramLogReportConfig::default()
    };
    assert!(matches!(
        generate_histogram_log_report([""].iter().copied(), &config),
        Err(DecodeError::InvalidLogLine(_))
    ));

    config = HistogramLogReportConfig {
        moving_window: Some(HistogramLogMovingWindowConfig {
            percentile_to_report: 101.0,
            length_sec: 1.0,
        }),
        ..HistogramLogReportConfig::default()
    };
    assert!(matches!(
        generate_histogram_log_report([""].iter().copied(), &config),
        Err(DecodeError::InvalidLogLine(_))
    ));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn concurrent_double_snapshot_log_line_roundtrip_preserves_counts() {
    use crate::concurrent::ConcurrentDoubleHistogram;

    let mut histogram = ConcurrentDoubleHistogram::builder()
        .highest_to_lowest_value_ratio(1024)
        .significant_digits(2)
        .build()
        .unwrap();
    histogram.meta_data_mut().set_tag_string("phase-a".to_string());
    let recorder = DoubleRecorder::from_histogram(histogram);
    recorder.record_value_with_count(1.5, 2).unwrap();
    recorder.record_value(12.0).unwrap();

    let sample = recorder.begin_interval_sample();
    let snapshot = sample.snapshot();
    let line = encode_concurrent_double_snapshot_log_line_with_max_value_unit_ratio(&snapshot, 40.0, 41.0, 1.0).unwrap();
    let record = decode_histogram_log_line(&line).unwrap().unwrap();

    match record {
        HistogramLogRecord::Interval(entry) => match entry.histogram {
            DecodedHistogram::Double(decoded) => {
                assert_eq!(Some("phase-a".to_string()), entry.tag);
                assert_eq!(snapshot.get_total_count(), decoded.get_total_count());
                assert_eq!(snapshot.get_count_at_value(1.5), decoded.get_count_at_value(1.5));
                assert_eq!(snapshot.get_count_at_value(12.0), decoded.get_count_at_value(12.0));
            }
            DecodedHistogram::Integer(_) => panic!("decoded concurrent double snapshot as integer histogram"),
        },
        _ => panic!("decoded concurrent double snapshot log line as non-interval record"),
    }
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_writer_accepts_concurrent_double_snapshot() {
    let recorder = DoubleRecorder::builder()
        .highest_to_lowest_value_ratio(1024)
        .significant_digits(2)
        .build()
        .unwrap();
    recorder.record_value(1.5).unwrap();

    let sample = recorder.begin_interval_sample();
    let snapshot = sample.snapshot();
    let mut output = Vec::new();
    {
        let mut writer = HistogramLogWriter::new(&mut output);
        writer.write_concurrent_double_snapshot_interval(&snapshot, 10.0, 11.0).unwrap();
    }
    let line = String::from_utf8(output).unwrap();
    assert!(line.contains("HIST"));
    assert!(matches!(
        decode_histogram_log_line(&line).unwrap().unwrap(),
        HistogramLogRecord::Interval(_)
    ));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_rejects_mixed_integer_and_double_logs() {
    let mut integer_histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    integer_histogram.record_value(100).unwrap();
    let mut double_histogram = DoubleHistogram::new(2).unwrap();
    double_histogram.record_value(1.5).unwrap();

    let log = format!(
        "{}{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&integer_histogram, 10.0, 11.0, 1.0).unwrap(),
        encode_double_histogram_log_line_with_max_value_unit_ratio(&double_histogram, 11.0, 12.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };

    assert!(matches!(
        generate_histogram_log_report(log.lines(), &config),
        Err(DecodeError::InvalidLogLine(_))
    ));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_can_correct_for_coordinated_omission() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value(100).unwrap();

    let log = format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        expected_interval_for_coordinated_omission_correction: 10.0,
        csv: true,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    assert!(report.interval_log.contains("1.000,10,"));
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_report_can_emit_text_output() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let log = format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
    );
    let config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        csv: false,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report(log.lines(), &config).unwrap();
    assert!(report.interval_log.starts_with("Time: IntervalPercentiles"));
    assert!(report.interval_log.contains("1.000: I:3"));
    assert!(report.percentile_distribution.contains("Value"));
    assert!(report.percentile_distribution.contains("#[Mean"));
}
