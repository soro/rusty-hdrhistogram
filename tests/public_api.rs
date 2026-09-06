use hdrhistogram::concurrent::{ConcurrentDoubleHistogram, SaturatingConcurrentDoubleHistogram};
use hdrhistogram::{
    DoubleHistogram, DoubleRecorder, SaturatingDoubleHistogram, SaturatingDoubleRecorder, SaturatingSingleWriterDoubleRecorder,
    SingleWriterDoubleRecorder,
};

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
