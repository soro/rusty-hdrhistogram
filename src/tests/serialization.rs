use bytes::{BytesMut, IntoBuf};
use concurrent;
use serialization::*;
use st;

#[test]
fn test_zig_zag() {
    let mut buf = BytesMut::with_capacity(30);
    let to_write_u = 102398084419879874;
    let to_write_neg = -100203;
    zigzag::put_u64(&mut buf, to_write_u).ok().unwrap();
    zigzag::put_i64(&mut buf, 0);
    zigzag::put_u64(&mut buf, 0).ok().unwrap();
    zigzag::put_i64(&mut buf, to_write_neg);

    let mut b = buf.into_buf();
    let read_u = zigzag::get_i64(&mut b) as u64;
    let read_0i = zigzag::get_i64(&mut b) as u64;
    let read_0u = zigzag::get_i64(&mut b) as u64;
    let read_neg = zigzag::get_i64(&mut b);

    assert_eq!(read_u, to_write_u);
    assert_eq!(read_0i, 0);
    assert_eq!(read_0u, 0);
    assert_eq!(
        read_neg,
        to_write_neg,
        "negative values should be read correctly"
    );
}

#[test]
fn test_simple_integer_histogram_encoding() {
    let mut histogram = st::Histogram::<u64>::with_high_sigvdig(274877906943, 3).unwrap();
    succ!(histogram.record_value(6147));
    succ!(histogram.record_value(1024));
    succ!(histogram.record_value(0));

    let result = histogram.serialize();

    assert!(result.is_ok());

    let (serialized, _) = result.unwrap();
    let deserialized = st::Histogram::<u64>::deserialize_from(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram == deserialized);

    succ!(histogram.record_value_with_count(100, 1 << 16));

    let (serialized, _) = histogram.serialize().unwrap();
    let deserialized = st::Histogram::<u64>::deserialize_from(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram == deserialized);
}

#[test]
fn test_atomic_histogram_encoding() {
    let mut histogram = concurrent::StaticHistogram::with_low_high_sigvdig(1, 274877906943, 3).unwrap();
    succ!(histogram.record_value(10000000));
    succ!(histogram.record_value(1024));
    succ!(histogram.record_value(0));
    succ!(histogram.record_value_with_count(500, 1 << 42));

    let (serialized, _) = histogram.as_snapshot().serialize().unwrap();
    let mut deserialized = concurrent::StaticHistogram::deserialize_from(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram.as_snapshot() == deserialized.as_snapshot());
}

#[test]
fn test_resizable_histogram_encoding() {
    let mut histogram = concurrent::ResizableHistogram::new(3).unwrap();
    succ!(histogram.record_value(10000000));
    succ!(histogram.record_value(1024));
    succ!(histogram.record_value(0));
    succ!(histogram.record_value_with_count(500, 1 << 42));

    let (serialized, _) = histogram.as_snapshot().serialize().unwrap();
    let mut deserialized = concurrent::ResizableHistogram::deserialize_from(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram.as_snapshot() == deserialized.as_snapshot());
}

#[test]
fn test_compressed_encoding() {
    let mut histogram = st::Histogram::<u64>::with_high_sigvdig(274877906943, 3).unwrap();
    succ!(histogram.record_value(6147));
    succ!(histogram.record_value(1024));
    succ!(histogram.record_value(0));
    succ!(histogram.record_value_with_count(100, 1 << 16));

    let (serialized, _) = histogram.serialize_compressed().unwrap();
    let deserialized = st::Histogram::<u64>::deserialize_from_compressed(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram == deserialized);
}

#[test]
fn test_ref_encoding() {
    let mut histogram = st::Histogram::<u64>::with_high_sigvdig(274877906943, 3).unwrap();
    succ!(histogram.record_value(6147));
    succ!(histogram.record_value(1024));
    succ!(histogram.record_value(0));
    succ!(histogram.record_value_with_count(100, 1 << 16));

    let (serialized, _) = (&mut histogram).serialize_compressed().unwrap();
    let deserialized = st::Histogram::<u64>::deserialize_from_compressed(&mut serialized.into_buf(), 0).unwrap();

    assert!(histogram == deserialized);
}
