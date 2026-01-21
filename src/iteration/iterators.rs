use crate::core::ReadableHistogram;
use crate::iteration::*;
use crate::iteration::histogram_iterator::HistogramIterator;
use crate::iteration::iteration_strategy::*;

/// Newtype wrappers for HistogramIterator with concrete strategies

pub struct AllValuesIterator<'a, T: 'a>(HistogramIterator<'a, T, AllValuesStrategy>);

impl<'a, T: 'a + ReadableHistogram> AllValuesIterator<'a, T> {
    pub fn new(histogram: &'a T) -> AllValuesIterator<'a, T> {
        let strategy = AllValuesStrategy { visited_index: -1 };
        let state = IterationState::new(histogram);
        AllValuesIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }
    pub fn reset(&mut self) {
        self.0.state.reset(self.0.histogram);
        self.0.strategy.visited_index = -1;
    }
}

// really need to make a derive macro for this
impl<'a, T: 'a + ReadableHistogram> Iterator for AllValuesIterator<'a, T> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct RecordedValuesIterator<'a, T: 'a>(HistogramIterator<'a, T, RecordedValuesStrategy>);

impl<'a, T: 'a + ReadableHistogram> RecordedValuesIterator<'a, T> {
    pub fn new(histogram: &T) -> RecordedValuesIterator<T> {
        let strategy = RecordedValuesStrategy { visited_index: -1 };
        let state = IterationState::new(histogram);
        RecordedValuesIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }
    pub fn reset(&mut self) {
        self.0.state.reset(self.0.histogram);
        self.0.strategy.visited_index = -1;
    }

    pub fn get_mean(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_mean_without_reset(iterator)
    }

    pub fn get_mean_without_reset(iterator: &mut Self) -> f64 {
        let histogram = iterator.0.histogram;
        let total_count = histogram.get_total_count();
        if total_count == 0 {
            return 0.0;
        }
        let mut total_value = 0;
        // TODO: switch to zero allocation version once implemented
        for value in iterator {
            total_value += histogram
                .settings()
                .median_equivalent_value(value.value_iterated_to) * value.count_at_value_iterated_to;
        }
        total_value as f64 / total_count as f64
    }

    pub fn get_std_deviation(iterator: &mut Self) -> f64 {
        iterator.reset();
        RecordedValuesIterator::get_std_deviation_without_reset(iterator)
    }

    pub fn get_std_deviation_without_reset(iterator: &mut Self) -> f64 {
        let histogram = iterator.0.histogram;
        let total_count = histogram.get_total_count();
        if total_count == 0 {
            return 0.0;
        }
        let mean = RecordedValuesIterator::get_mean_without_reset(iterator);
        iterator.reset();
        let mut geometric_deviation_total = 0.0;
        // TODO: switch to 0 alloc
        for value in iterator {
            let deviation = histogram
                .settings()
                .median_equivalent_value(value.value_iterated_to) as f64 - mean;
            geometric_deviation_total += (deviation * deviation) * value.count_added_in_this_iteration_step as f64;
        }
        (geometric_deviation_total / total_count as f64).sqrt()
    }
}

impl<'a, T: 'a + ReadableHistogram> Iterator for RecordedValuesIterator<'a, T> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct LinearIterator<'a, T: 'a>(HistogramIterator<'a, T, LinearStrategy>);

impl<'a, T: 'a + ReadableHistogram> LinearIterator<'a, T> {
    pub fn new(histogram: &T, value_units_per_bucket: u64) -> LinearIterator<T> {
        let highest_level = value_units_per_bucket - 1;
        let strategy = LinearStrategy {
            value_units_per_bucket,
            current_step_highest_value_reporting_level: highest_level,
            current_step_lowest_value_reporting_level: histogram.settings().lowest_equivalent_value(highest_level),
        };
        let state = IterationState::new(histogram);
        LinearIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }
    pub fn reset(&mut self, value_units_per_bucket: u64) {
        self.0.state.reset(self.0.histogram);

        let strategy = &mut self.0.strategy;

        let highest_level = value_units_per_bucket - 1;
        strategy.value_units_per_bucket = value_units_per_bucket;
        strategy.current_step_highest_value_reporting_level = highest_level;
        strategy.current_step_lowest_value_reporting_level = self.0
            .histogram
            .settings()
            .lowest_equivalent_value(highest_level);
    }
}

impl<'a, T: 'a + ReadableHistogram> Iterator for LinearIterator<'a, T> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct LogarithmicIterator<'a, T: 'a>(HistogramIterator<'a, T, LogarithmicStrategy>);

impl<'a, T: 'a + ReadableHistogram> LogarithmicIterator<'a, T> {
    pub fn new(histogram: &T, value_units_in_first_bucket: u64, log_base: f64) -> LogarithmicIterator<T> {
        let hvrl = value_units_in_first_bucket - 1;
        let strategy = LogarithmicStrategy {
            value_units_in_first_bucket,
            log_base,
            next_value_reporting_level: value_units_in_first_bucket as f64,
            current_step_highest_value_reporting_level: hvrl,
            current_step_lowest_value_reporting_level: histogram.settings().lowest_equivalent_value(hvrl),
        };
        let state = IterationState::new(histogram);
        LogarithmicIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }

    pub fn reset(&mut self, value_units_in_first_bucket: u64, log_base: f64) {
        let histogram = self.0.histogram;
        self.0.state.reset(histogram);

        let hvrl = value_units_in_first_bucket - 1;
        let strategy = &mut self.0.strategy;

        strategy.value_units_in_first_bucket = value_units_in_first_bucket;
        strategy.log_base = log_base;
        strategy.next_value_reporting_level = value_units_in_first_bucket as f64;
        strategy.current_step_highest_value_reporting_level = hvrl;
        strategy.current_step_lowest_value_reporting_level = histogram.settings().lowest_equivalent_value(hvrl);
    }
}

impl<'a, T: 'a + ReadableHistogram> Iterator for LogarithmicIterator<'a, T> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}

pub struct PercentileIterator<'a, T: 'a>(HistogramIterator<'a, T, PercentileStrategy>);

impl<'a, T: 'a + ReadableHistogram> PercentileIterator<'a, T> {
    pub fn new(histogram: &T, percentile_ticks_per_half_distance: u32) -> PercentileIterator<T> {
        let strategy = PercentileStrategy {
            percentile_ticks_per_half_distance: percentile_ticks_per_half_distance as isize,
            percentile_level_to_iterate_to: 0.0,
            percentile_level_to_iterate_from: 0.0,
            reached_last_recorded_value: false,
        };
        let state = IterationState::new(histogram);
        PercentileIterator(HistogramIterator {
            histogram,
            state,
            strategy,
        })
    }
    pub fn reset(&mut self, percentile_ticks_per_half_distance: u32) {
        self.0.state.reset(self.0.histogram);

        let strategy = &mut self.0.strategy;

        strategy.percentile_ticks_per_half_distance = percentile_ticks_per_half_distance as isize;
        strategy.percentile_level_to_iterate_to = 0.0;
        strategy.percentile_level_to_iterate_from = 0.0;
        strategy.reached_last_recorded_value = false;
    }
}

impl<'a, T: 'a + ReadableHistogram> Iterator for PercentileIterator<'a, T> {
    type Item = IterationValue;
    fn next(&mut self) -> Option<IterationValue> {
        self.0.next_value()
    }
}
