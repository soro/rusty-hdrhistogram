use crate::concurrent::{DoubleRecorder, ResizableConcurrentHistogram};
use crate::core::histogram_settings::{HistogramSettings, V2_ENCODING_HEADER_SIZE, V2_ENCODING_MAX_WORD_SIZE_IN_BYTES};
use crate::encoding::*;
use crate::st::{DoubleHistogram, Histogram};
#[cfg(feature = "encoding-compression")]
use flate2::{read::ZlibEncoder, Compression};
#[cfg(feature = "encoding-base64")]
use std::io::Cursor;
#[cfg(feature = "encoding-compression")]
use std::io::Read;

crate::static_histogram! {
    type EncodingStaticHistogram = {
        lowest_discernible_value: 1,
        highest_trackable_value: 3_600_000_000,
        significant_digits: 3,
    };
}

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
fn static_histogram_v2_roundtrip_preserves_counts() {
    let mut histogram = EncodingStaticHistogram::new();
    histogram.record_value_with_count(0, 3).unwrap();
    histogram.record_value(1).unwrap();
    histogram.record_value_with_count(10_000, 2).unwrap();
    histogram.record_value_with_count(123_456_789, 4).unwrap();

    let encoded = encode_histogram_v2(&histogram).unwrap();
    assert_eq!(&encoded[..4], &V2_ENCODING_COOKIE.to_be_bytes());

    let decoded = decode_histogram_v2(&encoded).unwrap();
    assert_eq!(histogram.counts_array_length(), decoded.counts_array_length());
    assert_eq!(histogram.get_highest_trackable_value(), decoded.get_highest_trackable_value());
    assert_eq!(
        histogram.get_number_of_significant_value_digits(),
        decoded.get_number_of_significant_value_digits()
    );
    assert_eq!(histogram.get_total_count(), decoded.get_total_count());
    assert_eq!(histogram.get_max_value(), decoded.get_max_value());
    assert_eq!(histogram.get_count_at_value(0), decoded.get_count_at_value(0));
    assert_eq!(histogram.get_count_at_value(10_000), decoded.get_count_at_value(10_000));
    assert_eq!(histogram.get_count_at_value(123_456_789), decoded.get_count_at_value(123_456_789));
}

#[test]
fn histogram_v2_decode_canonicalizes_aligned_normalizing_index_offsets() {
    let mut histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let mut encoded = encode_histogram_v2(&histogram).unwrap();
    let half_bucket = histogram.settings().sub_bucket_half_count as i32;
    for offset in [0, half_bucket, -half_bucket, i32::MAX / half_bucket * half_bucket, i32::MIN] {
        encoded[8..12].copy_from_slice(&offset.to_be_bytes());

        let mut decoded = decode_histogram_v2(&encoded).unwrap();
        assert_eq!(3, decoded.get_total_count());
        assert_eq!(Some(3), decoded.get_count_at_value(100));
        // Exercise the low-half-bucket path that relies on decoded alignment.
        decoded.shift_values_left(1).unwrap();
        assert_eq!(3, decoded.get_total_count());
        assert_eq!(Some(3), decoded.get_count_at_value(200));
    }
}

#[test]
fn histogram_v2_decode_rejects_misaligned_normalizing_index_offsets() {
    let histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    let mut encoded = encode_histogram_v2(&histogram).unwrap();
    for offset in [1_i32, -1, i32::MAX, i32::MIN + 1] {
        encoded[8..12].copy_from_slice(&offset.to_be_bytes());
        assert!(matches!(decode_histogram_v2(&encoded), Err(DecodeError::InvalidPayload)));
    }
}

#[test]
fn histogram_v1_decode_checks_normalizing_index_offset_alignment() {
    let histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    let mut encoded = encode_histogram_v2(&histogram).unwrap();
    encoded.truncate(V2_ENCODING_HEADER_SIZE);
    encoded[..4].copy_from_slice(&(0x1c849301_u32 | 0x80).to_be_bytes()); // V1, eight-byte counts.
    encoded[4..8].copy_from_slice(&16_i32.to_be_bytes());
    encoded.extend_from_slice(&0_i64.to_be_bytes());
    encoded.extend_from_slice(&3_i64.to_be_bytes());
    assert_eq!(Some(3), decode_histogram(&encoded).unwrap().get_count_at_value(1));

    encoded[8..12].copy_from_slice(&1_i32.to_be_bytes());
    assert!(matches!(decode_histogram(&encoded), Err(DecodeError::InvalidPayload)));
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_histogram_decode_checks_normalizing_index_offset_alignment() {
    let histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    let mut raw = encode_histogram_v2(&histogram).unwrap();
    raw[8..12].copy_from_slice(&1_i32.to_be_bytes());
    let encoded = compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &zlib_compress(&raw));
    assert!(matches!(decode_histogram_compressed(&encoded), Err(DecodeError::InvalidPayload)));
}

#[test]
fn double_histogram_decode_rejects_offset_that_would_break_range_shifts() {
    let mut integer = Histogram::with_high_sigvdig(8_191, 3).unwrap();
    integer.record_value(1).unwrap();
    let mut inner = encode_histogram_v2(&integer).unwrap();
    inner[8..12].copy_from_slice(&1_i32.to_be_bytes());
    let mut encoded = DOUBLE_HISTOGRAM_ENCODING_COOKIE.to_be_bytes().to_vec();
    encoded.extend_from_slice(&3_i32.to_be_bytes());
    encoded.extend_from_slice(&2_i64.to_be_bytes());
    encoded.extend_from_slice(&inner);

    // Accepting this offset used to allow record_value(512.0) on the decoded
    // double histogram to read past the count allocation while shifting it.
    assert!(matches!(decode_double_histogram_v2(&encoded), Err(DecodeError::InvalidPayload)));
}

#[test]
fn double_histogram_decode_validates_derived_ranges_and_reciprocals() {
    let histogram = DoubleHistogram::new();
    let mut encoded = encode_double_histogram_v2(&histogram).unwrap();
    for ratio in [f64::MAX, f64::MAX / 1_024.0, f64::from_bits(1), 1e-320] {
        // Double framing is 16 bytes; the inner V2 ratio starts at byte 32.
        encoded[48..56].copy_from_slice(&ratio.to_be_bytes());
        assert!(matches!(
            decode_double_histogram_v2(&encoded),
            Err(DecodeError::DoubleCreation(crate::DoubleCreationError::InternalHistogramMismatch))
        ));
        #[cfg(feature = "encoding-compression")]
        {
            let mut compressed = DOUBLE_HISTOGRAM_COMPRESSED_ENCODING_COOKIE.to_be_bytes().to_vec();
            compressed.extend_from_slice(&encoded[4..16]);
            compressed.extend_from_slice(&compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &zlib_compress(&encoded[16..])));
            assert!(matches!(
                decode_double_histogram_compressed(&compressed),
                Err(DecodeError::DoubleCreation(crate::DoubleCreationError::InternalHistogramMismatch))
            ));
        }
    }
    // Subnormal conversion ratios with finite reciprocals remain valid.
    for ratio in [f64::MIN_POSITIVE / 2.0, f64::MIN_POSITIVE, 1.0] {
        encoded[48..56].copy_from_slice(&ratio.to_be_bytes());
        let mut decoded = decode_double_histogram_v2(&encoded).unwrap();
        let value = decoded.get_current_lowest_trackable_non_zero_value() * 2.0;
        decoded.record_value(value).unwrap();
        assert_eq!(1, decoded.get_count_at_value(value));
        assert!(decoded.get_max_value().is_finite());
    }
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_headers_reject_non_finite_timestamps() {
    for header in ["StartTime", "BaseTime"] {
        for value in ["NaN", "inf", "-inf"] {
            let line = format!("#[{header}: {value} (seconds since epoch)]");
            assert!(matches!(scan_histogram_log_line(&line), Err(DecodeError::InvalidLogLine(_))));
            assert!(matches!(decode_histogram_log_line(&line), Err(DecodeError::InvalidLogLine(_))));
            let mut scanner = HistogramLogScanner::new(Cursor::new(line.as_bytes()));
            assert!(matches!(scanner.next_record(), Err(DecodeError::InvalidLogLine(_))));
            assert!(!scanner.observed_start_time());
            assert!(!scanner.observed_base_time());
            assert!(matches!(
                generate_histogram_log_report([line.as_str()], &HistogramLogReportConfig::default()),
                Err(DecodeError::InvalidLogLine(_))
            ));
        }
    }
}

#[test]
fn decoded_double_range_growth_retains_low_bound_without_precision_loss() {
    let histogram = DoubleHistogram::builder().significant_digits(0).build().unwrap();
    let mut encoded = encode_double_histogram_v2(&histogram).unwrap();
    let ratio = f64::from_bits(f64::MIN_POSITIVE.to_bits() + 1);
    encoded[48..56].copy_from_slice(&ratio.to_be_bytes());
    let mut decoded = decode_double_histogram_v2(&encoded).unwrap();
    let low = decoded.get_current_lowest_trackable_non_zero_value();
    let high = low * 1_024.0;
    decoded.record_value(low).unwrap();
    // Dividing the retained low bound before scaling it back would round away
    // its low bit in the subnormal intermediate value.
    decoded.record_value(high).unwrap();
    assert_eq!(low, decoded.get_current_lowest_trackable_non_zero_value());
    assert_eq!(1, decoded.get_count_at_value(low));
    assert_eq!(1, decoded.get_count_at_value(high));
    assert_eq!(&ratio.to_be_bytes(), &encode_double_histogram_v2(&decoded).unwrap()[48..56]);
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_scanner_rejects_derived_timestamp_overflow() {
    for (start, base, interval_start, length) in [
        (0.0, f64::MAX, f64::MAX, 0.0),  // Absolute start overflows.
        (0.0, f64::MAX, 0.0, f64::MAX),  // Absolute end overflows.
        (-f64::MAX, 0.0, f64::MAX, 0.0), // Relative start/end overflow.
        (f64::MAX, 0.0, -f64::MAX, 0.0),
    ] {
        let log = format!("#[StartTime: {start}]\n#[BaseTime: {base}]\n{interval_start},{length},0,unused\n");
        let mut scanner = HistogramLogScanner::new(Cursor::new(log));
        assert!(matches!(scanner.next_interval(), Err(DecodeError::InvalidLogLine(_))));
    }

    let histogram = Histogram::builder().build().unwrap();
    assert!(matches!(
        encode_histogram_log_line(&histogram, -f64::MAX, f64::MAX),
        Err(EncodeError::InvalidLogLine(_))
    ));
    assert!(matches!(
        scan_histogram_log_line(&format!("{0},{0},0,unused", f64::MAX)),
        Err(DecodeError::InvalidLogLine(_))
    ));
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
    let mut histogram = DoubleHistogram::with_significant_digits(3).unwrap();
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
fn compressed_histogram_decode_rejects_trailing_inflated_bytes() {
    let mut histogram = Histogram::with_high_sigvdig(1_000, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();

    let mut raw = encode_histogram_v2(&histogram).unwrap();
    raw.resize(raw.len() + 1024 * 1024, 0);
    let compressed = zlib_compress(&raw);
    let encoded = compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &compressed);

    assert!(matches!(decode_histogram_compressed(&encoded), Err(DecodeError::InvalidPayload)));
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_histogram_requires_complete_checked_zlib_stream() {
    let mut histogram = Histogram::with_high_sigvdig(1_000, 2).unwrap();
    histogram.record_value_with_count(100, 3).unwrap();
    let raw = encode_histogram_v2(&histogram).unwrap();

    for level in [0, 1, 6, 9] {
        let encoded = encode_histogram_compressed_with_level(&histogram, level).unwrap();
        let compressed = &encoded[8..];
        assert_eq!(Some(3), decode_histogram_compressed(&encoded).unwrap().get_count_at_value(100));

        // Keep the outer frame length valid: this specifically tests truncated
        // zlib contents, not the frame reader's existing length checks.
        for length in 0..compressed.len() {
            let truncated = compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &compressed[..length]);
            assert!(
                decode_histogram_compressed(&truncated).is_err(),
                "level {level}, compressed length {length}"
            );
        }
        let mut corrupt = compressed.to_vec();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(matches!(
            decode_histogram_compressed(&compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &corrupt)),
            Err(DecodeError::Compression(_))
        ));

        let mut trailing = compressed.to_vec();
        trailing.extend_from_slice(&[1, 2, 3]);
        assert!(matches!(
            decode_histogram_compressed(&compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &trailing)),
            Err(DecodeError::InvalidPayload)
        ));
        let concatenated = [compressed, compressed].concat();
        assert!(matches!(
            decode_histogram_compressed(&compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &concatenated)),
            Err(DecodeError::InvalidPayload)
        ));
    }

    let mut empty = raw[..V2_ENCODING_HEADER_SIZE].to_vec();
    empty[4..8].copy_from_slice(&0_i32.to_be_bytes());
    let encoded_empty = compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &zlib_compress(&empty));
    assert_eq!(0, decode_histogram_compressed(&encoded_empty).unwrap().get_total_count());
}

#[cfg(feature = "encoding-compression")]
#[test]
fn legacy_compressed_histograms_also_require_stream_completion() {
    let histogram = Histogram::with_high_sigvdig(1_024, 2).unwrap();
    let mut v1 = encode_histogram_v2(&histogram).unwrap();
    v1.truncate(V2_ENCODING_HEADER_SIZE);
    v1[..4].copy_from_slice(&(0x1c849301_u32 | 0x80).to_be_bytes());
    v1[4..8].copy_from_slice(&16_i32.to_be_bytes());
    v1.extend_from_slice(&0_i64.to_be_bytes());
    v1.extend_from_slice(&3_i64.to_be_bytes());

    let mut v0 = (0x1c849308_u32 | 0x80).to_be_bytes().to_vec();
    v0.extend_from_slice(&2_i32.to_be_bytes());
    v0.extend_from_slice(&1_i64.to_be_bytes());
    v0.extend_from_slice(&1_024_i64.to_be_bytes());
    v0.extend_from_slice(&3_i64.to_be_bytes());
    v0.extend_from_slice(&0_i64.to_be_bytes());
    v0.extend_from_slice(&3_i64.to_be_bytes());

    for (cookie, raw) in [(0x1c849302_u32 | 0x80, v1), (0x1c849309_u32 | 0x80, v0)] {
        let compressed = zlib_compress(&raw);
        let encoded = compressed_frame(cookie, &compressed);
        assert_eq!(Some(3), decode_histogram_compressed(&encoded).unwrap().get_count_at_value(1));
        for cut in 1..=4 {
            let truncated = compressed_frame(cookie, &compressed[..compressed.len() - cut]);
            assert!(decode_histogram_compressed(&truncated).is_err());
        }
        let mut corrupt = compressed;
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(matches!(
            decode_histogram_compressed(&compressed_frame(cookie, &corrupt)),
            Err(DecodeError::Compression(_))
        ));
    }
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_double_histogram_rejects_truncated_inner_stream() {
    let mut histogram = DoubleHistogram::new();
    histogram.record_value(12.0).unwrap();
    let encoded = encode_double_histogram_compressed(&histogram).unwrap();
    for cut in 1..=4 {
        let mut truncated = encoded[..encoded.len() - cut].to_vec();
        let compressed_length = (truncated.len() - 24) as i32;
        truncated[20..24].copy_from_slice(&compressed_length.to_be_bytes());
        assert!(decode_double_histogram_compressed(&truncated).is_err());
    }
}

fn assert_auto_grown_double_roundtrip(digits: u8, initial_ratio: u64, high_value: f64) {
    // Stay off a power-of-two range boundary: the initial powi-based scaling
    // has platform-dependent precision (also exercised by Miri).
    let low_value = 1.5;
    let mut histogram = DoubleHistogram::builder()
        .significant_digits(digits)
        .highest_to_lowest_value_ratio(initial_ratio)
        .auto_resize(true)
        .build()
        .unwrap();
    histogram.record_value(low_value).unwrap();
    histogram.record_value(high_value).unwrap();
    let ratio = histogram.get_highest_to_lowest_value_ratio();
    assert!((ratio as u128) * 10_u128.pow(digits as u32) >= (1_u128 << 61));
    // Decoding is deliberately more permissive than construction here.
    assert!(DoubleHistogram::builder()
        .significant_digits(digits)
        .highest_to_lowest_value_ratio(ratio)
        .build()
        .is_err());
    let raw = encode_double_histogram_v2(&histogram).unwrap();
    let mut decoded = decode_double_histogram_v2(&raw).unwrap();
    assert_eq!(ratio, decoded.get_highest_to_lowest_value_ratio());
    assert_eq!(2, decoded.get_total_count());
    assert_eq!(1, decoded.get_count_at_value(low_value));
    assert_eq!(1, decoded.get_count_at_value(high_value));
    // The accepted layout must remain usable, not just inspectable.
    decoded.record_value(low_value).unwrap();
    assert_eq!(2, decoded.get_count_at_value(low_value));

    #[cfg(feature = "encoding-compression")]
    {
        let encoded = encode_double_histogram_compressed(&histogram).unwrap();
        let decoded = decode_double_histogram_compressed(&encoded).unwrap();
        assert_eq!(2, decoded.get_total_count());
        assert_eq!(1, decoded.get_count_at_value(high_value));
    }
}

#[test]
fn double_decode_accepts_auto_grown_non_power_of_two_ratio() {
    assert_auto_grown_double_roundtrip(3, 3, 5e15);
}

#[test]
fn double_decode_accepts_auto_grown_zero_digit_layout() {
    assert_auto_grown_double_roundtrip(0, 2, 5e18);
}

#[test]
fn double_decode_rejects_unrepresentable_or_mismatched_layouts() {
    let mut histogram = DoubleHistogram::new();
    histogram.record_value(1.0).unwrap();
    let mut raw = encode_double_histogram_v2(&histogram).unwrap();
    for ratio in [0_i64, 1, 1_i64 << 62, i64::MAX] {
        raw[8..16].copy_from_slice(&ratio.to_be_bytes());
        assert!(matches!(decode_double_histogram_v2(&raw), Err(DecodeError::DoubleCreation(_))));
    }
    // A representable ratio must still agree with the encoded integer layout.
    raw[8..16].copy_from_slice(&1_024_i64.to_be_bytes());
    assert!(matches!(
        decode_double_histogram_v2(&raw),
        Err(DecodeError::DoubleCreation(
            crate::core::DoubleCreationError::InternalHistogramMismatch
        ))
    ));
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_histogram_decode_rejects_payload_length_larger_than_layout_capacity() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&V2_ENCODING_COOKIE.to_be_bytes());
    raw.extend_from_slice(&i32::MAX.to_be_bytes());
    raw.extend_from_slice(&0_i32.to_be_bytes());
    raw.extend_from_slice(&2_i32.to_be_bytes());
    raw.extend_from_slice(&1_i64.to_be_bytes());
    raw.extend_from_slice(&1_024_i64.to_be_bytes());
    raw.extend_from_slice(&1.0_f64.to_be_bytes());

    let compressed = zlib_compress(&raw);
    let encoded = compressed_frame(V2_COMPRESSED_ENCODING_COOKIE, &compressed);

    assert!(matches!(decode_histogram_compressed(&encoded), Err(DecodeError::InvalidPayload)));
}

#[cfg(feature = "encoding-compression")]
fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(raw, Compression::default());
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed).unwrap();
    compressed
}

#[cfg(feature = "encoding-compression")]
fn compressed_frame(cookie: u32, compressed: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(8 + compressed.len());
    encoded.extend_from_slice(&cookie.to_be_bytes());
    encoded.extend_from_slice(&(compressed.len() as i32).to_be_bytes());
    encoded.extend_from_slice(compressed);
    encoded
}

#[cfg(feature = "encoding-compression")]
#[test]
fn compressed_double_histogram_roundtrip_preserves_counts() {
    let mut histogram = DoubleHistogram::with_significant_digits(3).unwrap();
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
fn histogram_log_line_rejects_empty_tag_on_encode() {
    let mut histogram = Histogram::with_high_sigvdig(1_000, 2).unwrap();
    histogram.record_value(100).unwrap();
    histogram.meta_data.set_tag_string(String::new());

    assert!(matches!(
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0),
        Err(EncodeError::InvalidLogLine(_))
    ));
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
fn histogram_log_scanner_rejects_non_finite_and_negative_interval_metadata() {
    for line in [
        "1.000,-1.000,42.000,not-base64",
        "1.000,1.000,NaN,not-base64",
        "inf,1.000,42.000,not-base64",
    ] {
        assert!(matches!(scan_histogram_log_line(line), Err(DecodeError::InvalidLogLine(_))));
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
fn histogram_log_raw_line_scanning_preserves_text_and_tracks_timing() {
    let start = "#[StartTime: 100.123456 (seconds since epoch)]\r\n";
    let base = "#[BaseTime: 100.123456 (seconds since epoch)]\n";
    let blank = " \t\r\n";
    let interval = "Tag=phase-a,0.123456,0.000001,1.234567,not-base64";
    let log = format!("{start}{base}{blank}{interval}");
    let mut scanner = HistogramLogScanner::new(Cursor::new(log));

    let line = scanner.next_line().unwrap().unwrap();
    assert_eq!(start, line.raw_line);
    assert_eq!(Some(HistogramLogScannedRecord::StartTime(100.123456)), line.record);
    // Mixing record-oriented and line-oriented reads shares one timing state.
    assert_eq!(
        Some(HistogramLogScannedRecord::BaseTime(100.123456)),
        scanner.next_record().unwrap()
    );
    let line = scanner.next_line().unwrap().unwrap();
    assert_eq!(blank, line.raw_line);
    assert_eq!(None, line.record);
    let line = scanner.next_line().unwrap().unwrap();
    assert_eq!(interval, line.raw_line);
    let Some(HistogramLogScannedRecord::Interval(record)) = line.record else {
        panic!("expected interval metadata");
    };
    assert_eq!(0.123456, record.start_timestamp_sec);
    assert_eq!(0.000001, record.interval_length_sec);
    assert_eq!(1.234567, record.max_value);
    assert!((record.relative_start_time_sec - 0.123456).abs() < 1e-12);
    assert!((record.absolute_start_time_sec - 100.246912).abs() < 1e-12);
    assert!(matches!(record.decode_histogram(), Err(DecodeError::Base64(_))));
    assert!(scanner.next_line().unwrap().is_none());
}

#[cfg(feature = "encoding-base64")]
#[test]
fn histogram_log_raw_line_scanning_still_validates_metadata() {
    for log in [
        "#[StartTime: NaN]\n",
        "#[BaseTime: inf]\n",
        "1,-1,0,unused\n",
        "1e308,1e308,0,unused\n",
    ] {
        let mut scanner = HistogramLogScanner::new(Cursor::new(log));
        assert!(matches!(scanner.next_line(), Err(DecodeError::InvalidLogLine(_))));
    }
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
    let mut histogram = DoubleHistogram::with_significant_digits(3).unwrap();
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
    let mut histogram = DoubleHistogram::with_significant_digits(3).unwrap();
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
fn histogram_log_report_stops_reading_at_first_interval_past_end() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value(100).unwrap();
    histogram.meta_data.set_tag_string("phase-a".to_string());

    let mut log = format!(
        "{}{}{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 11.0, 12.0, 1.0).unwrap(),
        "Tag=other,12,1,0,not-base64\n",
    );
    let expected_position = log.len() as u64;
    log.push_str("malformed discarded tail\n");
    let mut reader = Cursor::new(log);
    let config = HistogramLogReportConfig {
        tag_filter: HistogramLogTagFilter::Tag("phase-a".to_string()),
        range_start_time_sec: 1.0,
        range_end_time_sec: 1.0,
        ..HistogramLogReportConfig::default()
    };

    let report = generate_histogram_log_report_from_reader(&mut reader, &config).unwrap();
    assert_eq!(1, report.processed_interval_count);
    assert_eq!(vec![Some("phase-a".to_string())], report.tags);
    // The stopping interval need not match the tag or have a decodable payload.
    // In particular, no subsequent records or bytes are requested from the reader.
    assert_eq!(expected_position, reader.position());
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
    let mut double_histogram = DoubleHistogram::with_significant_digits(2).unwrap();
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
fn histogram_log_report_truncates_integer_coordinated_omission_interval_like_java() {
    let mut histogram = Histogram::with_high_sigvdig(10_000, 2).unwrap();
    histogram.record_value(100).unwrap();

    let log = format!(
        "{}{}",
        histogram_log_start_time_line(10.0),
        encode_histogram_log_line_with_max_value_unit_ratio(&histogram, 10.0, 11.0, 1.0).unwrap(),
    );

    let fractional_interval_config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        expected_interval_for_coordinated_omission_correction: 50.7,
        csv: true,
        ..HistogramLogReportConfig::default()
    };
    let report = generate_histogram_log_report(log.lines(), &fractional_interval_config).unwrap();
    assert!(report.interval_log.contains("1.000,2,"));

    let sub_unit_interval_config = HistogramLogReportConfig {
        output_value_unit_ratio: 1.0,
        expected_interval_for_coordinated_omission_correction: 0.5,
        csv: true,
        ..HistogramLogReportConfig::default()
    };
    let report = generate_histogram_log_report(log.lines(), &sub_unit_interval_config).unwrap();
    assert!(report.interval_log.contains("1.000,1,"));
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
