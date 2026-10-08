use crate::core::RecordError;

/// Record only the synthetic samples, after the caller records the actual
/// measurement. Keep validation off ordinary (uncorrected) recording paths.
pub(crate) fn record_missing_double_values(
    value: f64,
    expected_interval: f64,
    mut record: impl FnMut(f64) -> Result<(), RecordError>,
) -> Result<(), RecordError> {
    if expected_interval <= 0.0 {
        return Ok(());
    }
    if !expected_interval.is_finite() {
        return Err(RecordError::InvalidExpectedInterval);
    }

    let mut previous = value;
    loop {
        let missing = previous - expected_interval;
        if missing < expected_interval {
            return Ok(());
        }
        // A subtraction may round back to the same value, even after an
        // earlier subtraction made progress (round-to-even halfway cases).
        if !missing.is_finite() || missing >= previous {
            return Err(RecordError::InvalidExpectedInterval);
        }
        record(missing)?;
        previous = missing;
    }
}
