use hdrhistogram::concurrent::{ConcurrentDoubleHistogram, SaturatingConcurrentDoubleHistogram};
use hdrhistogram::{
    DoubleHistogram, DoubleRecorder, SaturatingDoubleHistogram, SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder,
    SingleWriterDoubleRecorder,
};

#[test]
fn public_capability_traits_support_generic_iteration_and_encoding() {
    use hdrhistogram::encoding::{decode_histogram_v2, encode_histogram_v2, EncodableHistogram};
    use hdrhistogram::iteration::{IterableHistogram, RecordedValuesIterator};

    fn recorded_count<H: IterableHistogram>(histogram: &H) -> u64 {
        let mut values = RecordedValuesIterator::new(histogram);
        values.reset();
        values.map(|value| value.count_at_value_iterated_to).sum()
    }

    fn roundtrip<H: EncodableHistogram>(histogram: &H) -> hdrhistogram::Histogram {
        decode_histogram_v2(&encode_histogram_v2(histogram).unwrap()).unwrap()
    }

    let mut histogram = hdrhistogram::Histogram::builder().significant_digits(2).build().unwrap();
    histogram.record_value_with_count(42, 3).unwrap();
    assert_eq!(3, recorded_count(&histogram));
    assert_eq!(3, roundtrip(&histogram).get_total_count());
}

#[test]
fn logarithmic_reporting_can_propagate_parameter_errors() {
    use hdrhistogram::iteration::{DoubleLogarithmicIterator, IterableHistogram, LogarithmicIterator};
    use hdrhistogram::{Histogram, IterationError};

    fn count_buckets<H: IterableHistogram>(histogram: &H, width: u64, base: f64) -> Result<u64, IterationError> {
        let mut values = LogarithmicIterator::new(histogram, width, base)?;
        values.reset(width, base)?;
        Ok(values.map(|value| value.count_added_in_this_iteration_step).sum())
    }

    let mut histogram = Histogram::builder().build().unwrap();
    // Even an empty reporter rejects invalid parameters without unwinding.
    assert_eq!(Err(IterationError::InvalidBucketWidth), count_buckets(&histogram, 0, 2.0));
    assert_eq!(Err(IterationError::InvalidLogBase), count_buckets(&histogram, 1, 1.0));
    histogram.record_value_with_count(42, 3).unwrap();
    assert_eq!(Ok(3), count_buckets(&histogram, 1, 1.5));

    assert_eq!(
        Some(IterationError::InvalidBucketWidth),
        DoubleLogarithmicIterator::new(&histogram, f64::NAN, 2.0).err()
    );
    let mut double_values = DoubleLogarithmicIterator::new(&histogram, 1.0, 2.0).unwrap();
    assert_eq!(Err(IterationError::InvalidLogBase), double_values.reset(1.0, 1.0));
    assert_eq!(3, double_values.map(|value| value.count_added_in_this_iteration_step).sum::<u64>());

    let recorder = DoubleRecorder::new();
    recorder.record_value(42.0).unwrap();
    let sample = recorder.begin_interval_sample();
    assert_eq!(
        Some(IterationError::InvalidLogBase),
        sample.logarithmic_bucket_values(1.0, 1.0).err()
    );
    assert_eq!(
        1,
        sample
            .logarithmic_bucket_values(1.0, 2.0)
            .unwrap()
            .map(|value| value.count_added_in_this_iteration_step)
            .sum::<u64>()
    );
}

#[test]
fn double_aliases_construct_without_policy_annotations() {
    let mut histogram = DoubleHistogram::new();
    histogram.record_value(42.0).unwrap();

    let mut saturating_histogram = SaturatingDoubleHistogram::new();
    saturating_histogram.record_value(f64::MAX).unwrap();

    let concurrent = ConcurrentDoubleHistogram::new();
    concurrent.record_value(42.0).unwrap();

    let saturating_concurrent = SaturatingConcurrentDoubleHistogram::new();
    saturating_concurrent.record_value(f64::MAX).unwrap();

    let recorder = DoubleRecorder::new();
    recorder.record_value(42.0).unwrap();

    let saturating_recorder = SaturatingDoubleRecorder::new();
    saturating_recorder.record_value(f64::MAX).unwrap();

    let (mut writer, _sampler) = SingleWriterDoubleRecorder::new();
    writer.record_value(42.0).unwrap();

    let (mut saturating_writer, _saturating_sampler) = SaturatingSingleWriterDoubleRecorder::new();
    saturating_writer.record_value(f64::MAX).unwrap();
}

#[test]
fn double_alias_builders_construct_without_result_annotations() {
    let _histogram = DoubleHistogram::builder().significant_digits(2).build().unwrap();
    let _saturating_histogram = SaturatingDoubleHistogram::builder().significant_digits(2).build().unwrap();
    let _concurrent = ConcurrentDoubleHistogram::builder().significant_digits(2).build().unwrap();
    let _saturating_concurrent = SaturatingConcurrentDoubleHistogram::builder()
        .significant_digits(2)
        .build()
        .unwrap();
    let _recorder = DoubleRecorder::builder().significant_digits(2).build().unwrap();
    let _saturating_recorder = SaturatingDoubleRecorder::builder().significant_digits(2).build().unwrap();
    let _handles = SingleWriterDoubleRecorder::builder().significant_digits(2).build().unwrap();
    let _saturating_handles = SaturatingSingleWriterDoubleRecorder::builder()
        .significant_digits(2)
        .build()
        .unwrap();
}
