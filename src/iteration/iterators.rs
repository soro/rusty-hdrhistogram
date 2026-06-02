use crate::core::{IterableHistogram, ReadableHistogram};
use crate::iteration::histogram_iterator::HistogramIterator;
use crate::iteration::iteration_state::IterationState;
use crate::iteration::iteration_strategy::*;
use crate::iteration::{DoubleIterationValue, IterationValue};

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
}

// really need to make a derive macro for this
impl<H: ReadableHistogram> Iterator for AllValuesIterator<H> {
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

    pub fn get_mean(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_mean_without_reset(iterator)
    }

    pub fn get_mean_without_reset(iterator: &mut Self) -> f64 {
        let total_count = iterator.0.histogram.get_total_count();
        if total_count == 0 {
            return 0.0;
        }
        let settings = iterator.0.histogram.settings();
        let mut total_value = 0;
        // TODO: switch to zero allocation version once implemented
        for value in iterator {
            total_value += settings.median_equivalent_value(value.value_iterated_to) * value.count_at_value_iterated_to;
        }
        total_value as f64 / total_count as f64
    }

    pub fn get_std_deviation(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_std_deviation_without_reset(iterator)
    }

    pub fn get_std_deviation_without_reset(iterator: &mut Self) -> f64 {
        let total_count = iterator.0.histogram.get_total_count();
        if total_count == 0 {
            return 0.0;
        }
        let mean = RecordedValuesIterator::get_mean_without_reset(iterator);
        iterator.reset();
        let settings = iterator.0.histogram.settings();
        let mut geometric_deviation_total = 0.0;
        // TODO: switch to 0 alloc
        for value in iterator {
            let deviation = settings.median_equivalent_value(value.value_iterated_to) as f64 - mean;
            geometric_deviation_total += (deviation * deviation) * value.count_added_in_this_iteration_step as f64;
        }
        (geometric_deviation_total / total_count as f64).sqrt()
    }
}

impl<H: ReadableHistogram> Iterator for RecordedValuesIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct LinearIterator<H>(HistogramIterator<H, LinearStrategy>);

impl<H: IterableHistogram> LinearIterator<H> {
    pub fn new(histogram: H, value_units_per_bucket: u64) -> LinearIterator<H> {
        LinearIterator::from_readable(histogram, value_units_per_bucket)
    }
}

impl<H: ReadableHistogram> LinearIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_per_bucket: u64) -> LinearIterator<H> {
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

    pub fn reset(&mut self, value_units_per_bucket: u64) {
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
}

impl<H: ReadableHistogram> Iterator for LinearIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct LogarithmicIterator<H>(HistogramIterator<H, LogarithmicStrategy>);

impl<H: IterableHistogram> LogarithmicIterator<H> {
    pub fn new(histogram: H, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<H> {
        LogarithmicIterator::from_readable(histogram, value_units_in_first_bucket, log_base)
    }
}

impl<H: ReadableHistogram> LogarithmicIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<H> {
        let hvrl = value_units_in_first_bucket - 1;
        let strategy = LogarithmicStrategy {
            value_units_in_first_bucket,
            log_base,
            next_value_reporting_level: value_units_in_first_bucket as f64,
            current_step_highest_value_reporting_level: hvrl,
            current_step_lowest_value_reporting_level: histogram.settings().lowest_equivalent_value(hvrl),
        };
        let state = IterationState::new(&histogram);
        LogarithmicIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self, value_units_in_first_bucket: u64, log_base: f64) {
        self.0.state.reset(&self.0.histogram);

        let hvrl = value_units_in_first_bucket - 1;
        let strategy = &mut self.0.strategy;

        strategy.value_units_in_first_bucket = value_units_in_first_bucket;
        strategy.log_base = log_base;
        strategy.next_value_reporting_level = value_units_in_first_bucket as f64;
        strategy.current_step_highest_value_reporting_level = hvrl;
        strategy.current_step_lowest_value_reporting_level = self.0.histogram.settings().lowest_equivalent_value(hvrl);
    }

    fn histogram(&self) -> &H {
        &self.0.histogram
    }
}

impl<H: ReadableHistogram> Iterator for LogarithmicIterator<H> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct PercentileIterator<H>(HistogramIterator<H, PercentileStrategy>);

impl<H: IterableHistogram> PercentileIterator<H> {
    pub fn new(histogram: H, percentile_ticks_per_half_distance: u32) -> PercentileIterator<H> {
        PercentileIterator::from_readable(histogram, percentile_ticks_per_half_distance)
    }
}

impl<H: ReadableHistogram> PercentileIterator<H> {
    pub(crate) fn from_readable(histogram: H, percentile_ticks_per_half_distance: u32) -> PercentileIterator<H> {
        let strategy = PercentileStrategy {
            percentile_ticks_per_half_distance: percentile_ticks_per_half_distance as isize,
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

    pub fn reset(&mut self, percentile_ticks_per_half_distance: u32) {
        self.0.state.reset(&self.0.histogram);

        let strategy = &mut self.0.strategy;

        strategy.percentile_ticks_per_half_distance = percentile_ticks_per_half_distance as isize;
        strategy.percentile_level_to_iterate_to = 0.0;
        strategy.percentile_level_to_iterate_from = 0.0;
        strategy.reached_last_recorded_value = false;
    }
}

impl<H: ReadableHistogram> Iterator for PercentileIterator<H> {
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
}

impl<H: ReadableHistogram> Iterator for DoubleAllValuesIterator<H> {
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
}

impl<H: ReadableHistogram> Iterator for DoubleRecordedValuesIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoubleLinearIterator<H>(LinearIterator<H>);

impl<H: IterableHistogram> DoubleLinearIterator<H> {
    pub fn new(histogram: H, value_units_per_bucket: f64) -> Self {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_per_bucket);
        DoubleLinearIterator(LinearIterator::new(histogram, integer_units))
    }
}

impl<H: ReadableHistogram> DoubleLinearIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_per_bucket: f64) -> Self {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_per_bucket);
        DoubleLinearIterator(LinearIterator::from_readable(histogram, integer_units))
    }

    pub fn reset(&mut self, value_units_per_bucket: f64) {
        let units = double_value_units_to_integer_units(self.0.histogram(), value_units_per_bucket);
        self.0.reset(units);
    }
}

impl<H: ReadableHistogram> Iterator for DoubleLinearIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoubleLogarithmicIterator<H>(LogarithmicIterator<H>);

impl<H: IterableHistogram> DoubleLogarithmicIterator<H> {
    pub fn new(histogram: H, value_units_in_first_bucket: f64, log_base: f64) -> Self {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_in_first_bucket);
        DoubleLogarithmicIterator(LogarithmicIterator::new(histogram, integer_units, log_base))
    }
}

impl<H: ReadableHistogram> DoubleLogarithmicIterator<H> {
    pub(crate) fn from_readable(histogram: H, value_units_in_first_bucket: f64, log_base: f64) -> Self {
        let integer_units = double_value_units_to_integer_units(&histogram, value_units_in_first_bucket);
        DoubleLogarithmicIterator(LogarithmicIterator::from_readable(histogram, integer_units, log_base))
    }

    pub fn reset(&mut self, value_units_in_first_bucket: f64, log_base: f64) {
        let units = double_value_units_to_integer_units(self.0.histogram(), value_units_in_first_bucket);
        self.0.reset(units, log_base);
    }
}

impl<H: ReadableHistogram> Iterator for DoubleLogarithmicIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

pub struct DoublePercentileIterator<H>(PercentileIterator<H>);

impl<H: IterableHistogram> DoublePercentileIterator<H> {
    pub fn new(histogram: H, percentile_ticks_per_half_distance: u32) -> Self {
        DoublePercentileIterator(PercentileIterator::new(histogram, percentile_ticks_per_half_distance))
    }
}

impl<H: ReadableHistogram> DoublePercentileIterator<H> {
    pub(crate) fn from_readable(histogram: H, percentile_ticks_per_half_distance: u32) -> Self {
        DoublePercentileIterator(PercentileIterator::from_readable(histogram, percentile_ticks_per_half_distance))
    }

    pub fn reset(&mut self, percentile_ticks_per_half_distance: u32) {
        self.0.reset(percentile_ticks_per_half_distance);
    }
}

impl<H: ReadableHistogram> Iterator for DoublePercentileIterator<H> {
    type Item = DoubleIterationValue;

    fn next(&mut self) -> Option<DoubleIterationValue> {
        self.0.next().map(DoubleIterationValue::from)
    }
}

fn double_value_units_to_integer_units<T: ReadableHistogram>(histogram: &T, value_units: f64) -> u64 {
    assert!(
        value_units.is_finite() && value_units > 0.0,
        "value units must be finite and positive"
    );
    let units = value_units / histogram.integer_to_double_value_conversion_ratio();
    assert!(units.is_finite() && units <= u64::MAX as f64, "value units are out of range");
    (units as u64).max(1)
}
