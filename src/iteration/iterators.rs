// Iterator constructors expose public capability traits, while their safe
// operations share the crate-private read interface with captured live views.
#![allow(private_bounds)]

use crate::core::{IterableHistogram, ReadableHistogram};
use crate::iteration::histogram_iterator::HistogramIterator;
use crate::iteration::iteration_error::IterationError;
use crate::iteration::iteration_state::IterationState;
use crate::iteration::iteration_strategy::*;
use crate::iteration::{DoubleIterationValue, IterationValue};
use std::num::{NonZeroU32, NonZeroU64};

/// Newtype wrappers for HistogramIterator with concrete strategies
pub struct AllValuesIterator<H>(HistogramIterator<H, AllValuesStrategy>);

impl<H: IterableHistogram> AllValuesIterator<H> {
    pub fn new(histogram: H) -> AllValuesIterator<H> {
        let strategy = AllValuesStrategy { visited_index: -1 };
        let state = IterationState::new(&histogram);
        AllValuesIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }
}

impl<H: ReadableHistogram> AllValuesIterator<H> {
    pub(crate) fn from_readable(histogram: H) -> AllValuesIterator<H> {
        let strategy = AllValuesStrategy { visited_index: -1 };
        let state = IterationState::new(&histogram);
        AllValuesIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self) {
        self.0.state.reset(&self.0.histogram);
        self.0.strategy.visited_index = -1;
    }

    pub fn try_next(&mut self) -> Result<Option<IterationValue>, IterationError> {
        self.0.try_next_value()
    }
}

// really need to make a derive macro for this
impl<H: IterableHistogram> Iterator for AllValuesIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct RecordedValuesIterator<H>(HistogramIterator<H, RecordedValuesStrategy>);

impl<H: IterableHistogram> RecordedValuesIterator<H> {
    pub fn new(histogram: H) -> RecordedValuesIterator<H> {
        RecordedValuesIterator::from_readable(histogram)
    }
}

impl<H: ReadableHistogram> RecordedValuesIterator<H> {
    pub(crate) fn from_readable(histogram: H) -> RecordedValuesIterator<H> {
        let strategy = RecordedValuesStrategy { visited_index: -1 };
        let state = IterationState::new(&histogram);
        RecordedValuesIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self) {
        self.0.state.reset(&self.0.histogram);
        self.0.strategy.visited_index = -1;
    }

    pub fn try_next(&mut self) -> Result<Option<IterationValue>, IterationError> {
        self.0.try_next_value()
    }

    pub fn try_get_mean(iterator: &mut Self) -> Result<f64, IterationError> {
        iterator.reset();
        RecordedValuesIterator::try_get_mean_without_reset(iterator)
    }

    pub fn try_get_mean_without_reset(iterator: &mut Self) -> Result<f64, IterationError> {
        iterator.0.check_not_concurrently_modified()?;
        let total_count = iterator.0.histogram.get_total_count();
        if total_count == 0 {
            return Ok(0.0);
        }
        let settings = iterator.0.histogram.settings();
        let mut total_value = 0;
        while let Some(value) = iterator.try_next()? {
            total_value += settings.median_equivalent_value(value.value_iterated_to) * value.count_at_value_iterated_to;
        }
        Ok(total_value as f64 / total_count as f64)
    }

    pub fn try_get_std_deviation(iterator: &mut Self) -> Result<f64, IterationError> {
        iterator.reset();
        RecordedValuesIterator::try_get_std_deviation_without_reset(iterator)
    }

    pub fn try_get_std_deviation_without_reset(iterator: &mut Self) -> Result<f64, IterationError> {
        iterator.0.check_not_concurrently_modified()?;
        let total_count = iterator.0.histogram.get_total_count();
        if total_count == 0 {
            return Ok(0.0);
        }
        let mean = RecordedValuesIterator::try_get_mean_without_reset(iterator)?;
        iterator.reset();
        let settings = iterator.0.histogram.settings();
        let mut geometric_deviation_total = 0.0;
        while let Some(value) = iterator.try_next()? {
            let deviation = settings.median_equivalent_value(value.value_iterated_to) as f64 - mean;
            geometric_deviation_total += (deviation * deviation) * value.count_added_in_this_iteration_step as f64;
        }
        Ok((geometric_deviation_total / total_count as f64).sqrt())
    }
}

impl<H: IterableHistogram> RecordedValuesIterator<H> {
    pub fn get_mean(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_mean_without_reset(iterator)
    }

    pub fn get_mean_without_reset(iterator: &mut Self) -> f64 {
        RecordedValuesIterator::try_get_mean_without_reset(iterator).expect("IterableHistogram sources must not be concurrently modified")
    }

    pub fn get_std_deviation(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_std_deviation_without_reset(iterator)
    }

    pub fn get_std_deviation_without_reset(iterator: &mut Self) -> f64 {
        RecordedValuesIterator::try_get_std_deviation_without_reset(iterator)
            .expect("IterableHistogram sources must not be concurrently modified")
    }
}

impl<H: IterableHistogram> Iterator for RecordedValuesIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

/// Linear buckets with a nonzero integer width.
///
/// ```compile_fail
/// use hdrhistogram::{Histogram, iteration::LinearIterator};
/// let histogram = Histogram::builder().build().unwrap();
/// let values = LinearIterator::new(&histogram, 0);
/// ```
pub struct LinearIterator<H>(HistogramIterator<H, LinearStrategy>);

impl<H: IterableHistogram> LinearIterator<H> {
    pub fn new(histogram: H, value_units_per_bucket: NonZeroU64) -> LinearIterator<H> {
        LinearIterator::from_readable(histogram, value_units_per_bucket)
    }
}

impl<H: ReadableHistogram> LinearIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_per_bucket: NonZeroU64) -> LinearIterator<H> {
        let value_units_per_bucket = value_units_per_bucket.get();
        let highest_level = value_units_per_bucket - 1;
        let strategy = LinearStrategy {
            value_units_per_bucket,
            current_step_highest_value_reporting_level: highest_level,
            current_step_lowest_value_reporting_level: histogram.settings().lowest_equivalent_value(highest_level),
        };
        let state = IterationState::new(&histogram);
        LinearIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self, value_units_per_bucket: NonZeroU64) {
        let value_units_per_bucket = value_units_per_bucket.get();
        self.0.state.reset(&self.0.histogram);

        let strategy = &mut self.0.strategy;

        let highest_level = value_units_per_bucket - 1;
        strategy.value_units_per_bucket = value_units_per_bucket;
        strategy.current_step_highest_value_reporting_level = highest_level;
        strategy.current_step_lowest_value_reporting_level = self.0.histogram.settings().lowest_equivalent_value(highest_level);
    }

    fn histogram(&self) -> &H {
        &self.0.histogram
    }

    pub fn try_next(&mut self) -> Result<Option<IterationValue>, IterationError> {
        self.0.try_next_value()
    }
}

impl<H: IterableHistogram> Iterator for LinearIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

/// Logarithmic buckets with a nonzero integer first-bucket width.
///
/// ```compile_fail
/// use hdrhistogram::{Histogram, iteration::LogarithmicIterator};
/// let histogram = Histogram::builder().build().unwrap();
/// let values = LogarithmicIterator::new(&histogram, 0, 2.0);
/// ```
pub struct LogarithmicIterator<H>(HistogramIterator<H, LogarithmicStrategy>);

fn validate_log_base(log_base: f64) -> Result<(), IterationError> {
    if !log_base.is_finite() || log_base <= 1.0 {
        return Err(IterationError::InvalidLogBase);
    }
    Ok(())
}

impl<H: IterableHistogram> LogarithmicIterator<H> {
    /// # Errors
    /// Returns [`IterationError::InvalidLogBase`] if `log_base` is non-finite
    /// or at most one. The first bucket width is nonzero by construction.
    pub fn new(histogram: H, value_units_in_first_bucket: NonZeroU64, log_base: f64) -> Result<LogarithmicIterator<H>, IterationError> {
        LogarithmicIterator::from_readable(histogram, value_units_in_first_bucket, log_base)
    }
}

impl<H: ReadableHistogram> LogarithmicIterator<H> {
    pub(crate) fn from_readable(
        histogram: H,
        value_units_in_first_bucket: NonZeroU64,
        log_base: f64,
    ) -> Result<LogarithmicIterator<H>, IterationError> {
        validate_log_base(log_base)?;
        let value_units_in_first_bucket = value_units_in_first_bucket.get();
        let hvrl = value_units_in_first_bucket - 1;
        let strategy = LogarithmicStrategy {
            value_units_in_first_bucket,
            log_base,
            next_value_reporting_level: value_units_in_first_bucket as f64,
            current_step_highest_value_reporting_level: hvrl,
            current_step_lowest_value_reporting_level: histogram.settings().lowest_equivalent_value(hvrl),
        };
        let state = IterationState::new(&histogram);
        Ok(LogarithmicIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        }))
    }

    /// # Errors
    /// Returns [`IterationError::InvalidLogBase`] if `log_base` is non-finite
    /// or at most one. An error leaves the iterator unchanged.
    pub fn reset(&mut self, value_units_in_first_bucket: NonZeroU64, log_base: f64) -> Result<(), IterationError> {
        validate_log_base(log_base)?;
        let value_units_in_first_bucket = value_units_in_first_bucket.get();
        self.0.state.reset(&self.0.histogram);

        let hvrl = value_units_in_first_bucket - 1;
        let strategy = &mut self.0.strategy;

        strategy.value_units_in_first_bucket = value_units_in_first_bucket;
        strategy.log_base = log_base;
        strategy.next_value_reporting_level = value_units_in_first_bucket as f64;
        strategy.current_step_highest_value_reporting_level = hvrl;
        strategy.current_step_lowest_value_reporting_level = self.0.histogram.settings().lowest_equivalent_value(hvrl);
        Ok(())
    }

    fn histogram(&self) -> &H {
        &self.0.histogram
    }

    pub fn try_next(&mut self) -> Result<Option<IterationValue>, IterationError> {
        self.0.try_next_value()
    }
}

impl<H: IterableHistogram> Iterator for LogarithmicIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

/// Percentile buckets with a nonzero number of ticks per half-distance.
///
/// ```compile_fail
/// use hdrhistogram::{Histogram, iteration::PercentileIterator};
/// let histogram = Histogram::builder().build().unwrap();
/// let values = PercentileIterator::new(&histogram, 0);
/// ```
pub struct PercentileIterator<H>(HistogramIterator<H, PercentileStrategy>);

impl<H: IterableHistogram> PercentileIterator<H> {
    pub fn new(histogram: H, percentile_ticks_per_half_distance: NonZeroU32) -> PercentileIterator<H> {
        PercentileIterator::from_readable(histogram, percentile_ticks_per_half_distance)
    }
}

impl<H: ReadableHistogram> PercentileIterator<H> {
    pub(crate) fn from_readable(histogram: H, percentile_ticks_per_half_distance: NonZeroU32) -> PercentileIterator<H> {
        let strategy = PercentileStrategy {
            percentile_ticks_per_half_distance,
            percentile_level_to_iterate_to: 0.0,
            percentile_level_to_iterate_from: 0.0,
            reached_last_recorded_value: false,
        };
        let state = IterationState::new(&histogram);
        PercentileIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self, percentile_ticks_per_half_distance: NonZeroU32) {
        self.0.state.reset(&self.0.histogram);

        let strategy = &mut self.0.strategy;

        strategy.percentile_ticks_per_half_distance = percentile_ticks_per_half_distance;
        strategy.percentile_level_to_iterate_to = 0.0;
        strategy.percentile_level_to_iterate_from = 0.0;
        strategy.reached_last_recorded_value = false;
    }

    pub fn try_next(&mut self) -> Result<Option<IterationValue>, IterationError> {
        self.0.try_next_value()
    }
}

impl<H: IterableHistogram> Iterator for PercentileIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct DoubleAllValuesIterator<H>(AllValuesIterator<H>);

impl<H: IterableHistogram> DoubleAllValuesIterator<H> {
    pub fn new(histogram: H) -> Self {
        DoubleAllValuesIterator(AllValuesIterator::new(histogram))
    }
}

impl<H: ReadableHistogram> DoubleAllValuesIterator<H> {
    pub(crate) fn from_readable(histogram: H) -> Self {
        DoubleAllValuesIterator(AllValuesIterator::from_readable(histogram))
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub fn try_next(&mut self) -> Result<Option<DoubleIterationValue>, IterationError> {
        self.0.try_next().map(|value| value.map(DoubleIterationValue::from))
    }
}

impl<H: IterableHistogram> Iterator for DoubleAllValuesIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoubleRecordedValuesIterator<H>(RecordedValuesIterator<H>);

impl<H: IterableHistogram> DoubleRecordedValuesIterator<H> {
    pub fn new(histogram: H) -> Self {
        DoubleRecordedValuesIterator(RecordedValuesIterator::new(histogram))
    }
}

impl<H: ReadableHistogram> DoubleRecordedValuesIterator<H> {
    pub(crate) fn from_readable(histogram: H) -> Self {
        DoubleRecordedValuesIterator(RecordedValuesIterator::from_readable(histogram))
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub fn try_next(&mut self) -> Result<Option<DoubleIterationValue>, IterationError> {
        self.0.try_next().map(|value| value.map(DoubleIterationValue::from))
    }
}

impl<H: IterableHistogram> Iterator for DoubleRecordedValuesIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoubleLinearIterator<H>(LinearIterator<H>);

impl<H: IterableHistogram> DoubleLinearIterator<H> {
    /// # Errors
    /// Returns [`IterationError::InvalidBucketWidth`] if the width is non-finite,
    /// non-positive, or too large in integer units. Positive widths smaller
    /// than one integer unit round up to one.
    pub fn new(histogram: H, value_units_per_bucket: f64) -> Result<Self, IterationError> {
        Self::from_readable(histogram, value_units_per_bucket)
    }
}

impl<H: ReadableHistogram> DoubleLinearIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_per_bucket: f64) -> Result<Self, IterationError> {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_per_bucket)?;
        Ok(DoubleLinearIterator(LinearIterator::from_readable(histogram, integer_units)))
    }

    /// # Errors
    /// Returns [`IterationError::InvalidBucketWidth`] if the width is non-finite,
    /// non-positive, or too large in integer units. An error leaves the iterator unchanged.
    pub fn reset(&mut self, value_units_per_bucket: f64) -> Result<(), IterationError> {
        let units = double_value_units_to_integer_units(self.0.histogram(), value_units_per_bucket)?;
        self.0.reset(units);
        Ok(())
    }

    pub fn try_next(&mut self) -> Result<Option<DoubleIterationValue>, IterationError> {
        self.0.try_next().map(|value| value.map(DoubleIterationValue::from))
    }
}

impl<H: IterableHistogram> Iterator for DoubleLinearIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoubleLogarithmicIterator<H>(LogarithmicIterator<H>);

impl<H: IterableHistogram> DoubleLogarithmicIterator<H> {
    /// # Errors
    /// Returns [`IterationError::InvalidBucketWidth`] if the first bucket width
    /// is non-finite, non-positive, or too large in integer units. Returns
    /// [`IterationError::InvalidLogBase`] if `log_base` is non-finite or at most
    /// one. Positive widths smaller than one integer unit round up to one.
    pub fn new(histogram: H, value_units_in_first_bucket: f64, log_base: f64) -> Result<Self, IterationError> {
        DoubleLogarithmicIterator::from_readable(histogram, value_units_in_first_bucket, log_base)
    }
}

impl<H: ReadableHistogram> DoubleLogarithmicIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_in_first_bucket: f64, log_base: f64) -> Result<Self, IterationError> {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_in_first_bucket)?;
        Ok(DoubleLogarithmicIterator(LogarithmicIterator::from_readable(
            histogram,
            integer_units,
            log_base,
        )?))
    }

    /// # Errors
    /// Returns [`IterationError::InvalidBucketWidth`] if the first bucket width
    /// is non-finite, non-positive, or too large in integer units. Returns
    /// [`IterationError::InvalidLogBase`] if `log_base` is non-finite or at most
    /// one. An error leaves the iterator unchanged.
    pub fn reset(&mut self, value_units_in_first_bucket: f64, log_base: f64) -> Result<(), IterationError> {
        let units = double_value_units_to_integer_units(self.0.histogram(), value_units_in_first_bucket)?;
        self.0.reset(units, log_base)
    }

    pub fn try_next(&mut self) -> Result<Option<DoubleIterationValue>, IterationError> {
        self.0.try_next().map(|value| value.map(DoubleIterationValue::from))
    }
}

impl<H: IterableHistogram> Iterator for DoubleLogarithmicIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoublePercentileIterator<H>(PercentileIterator<H>);

impl<H: IterableHistogram> DoublePercentileIterator<H> {
    pub fn new(histogram: H, percentile_ticks_per_half_distance: NonZeroU32) -> Self {
        DoublePercentileIterator(PercentileIterator::new(histogram, percentile_ticks_per_half_distance))
    }
}

impl<H: ReadableHistogram> DoublePercentileIterator<H> {
    pub(crate) fn from_readable(histogram: H, percentile_ticks_per_half_distance: NonZeroU32) -> Self {
        DoublePercentileIterator(PercentileIterator::from_readable(histogram, percentile_ticks_per_half_distance))
    }

    pub fn reset(&mut self, percentile_ticks_per_half_distance: NonZeroU32) {
        self.0.reset(percentile_ticks_per_half_distance);
    }

    pub fn try_next(&mut self) -> Result<Option<DoubleIterationValue>, IterationError> {
        self.0.try_next().map(|value| value.map(DoubleIterationValue::from))
    }
}

impl<H: IterableHistogram> Iterator for DoublePercentileIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

fn double_value_units_to_integer_units<T: ReadableHistogram>(histogram: &T, value_units: f64) -> Result<NonZeroU64, IterationError> {
    if !value_units.is_finite() || value_units <= 0.0 {
        return Err(IterationError::InvalidBucketWidth);
    }
    let units = value_units / histogram.integer_to_double_value_conversion_ratio();
    if !units.is_finite() || units > u64::MAX as f64 {
        return Err(IterationError::InvalidBucketWidth);
    }
    Ok(NonZeroU64::new(units as u64).unwrap_or(NonZeroU64::MIN))
}
