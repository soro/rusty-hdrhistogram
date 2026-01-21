use crate::core::ReadableHistogram;
use crate::iteration::*;

pub trait IterationStrategy<T: ReadableHistogram>: Sized {
    fn reached_iteration_level(&mut self, state: &IterationState, histogram: &T) -> bool;

    fn increment_iteration_level(&mut self, state: &IterationState, histogram: &T);

    fn has_next(&mut self, state: &IterationState, _histogram: &T) -> bool {
        default_has_next(state)
    }

    fn get_value_iterated_to(&mut self, state: &IterationState, histogram: &T) -> u64 {
        histogram
            .settings()
            .highest_equivalent_value(state.current_value_at_index)
    }
    fn get_percentile_iterated_to(&mut self, state: &IterationState) -> f64 {
        100.0 * (state.total_count_to_current_index as f64 / state.array_total_count as f64)
    }
}

fn default_has_next(state: &IterationState) -> bool {
    state.total_count_to_current_index < state.array_total_count
}

pub struct AllValuesStrategy {
    pub(in crate::iteration) visited_index: isize,
}

impl<T: ReadableHistogram> IterationStrategy<T> for AllValuesStrategy {
    fn reached_iteration_level(&mut self, state: &IterationState, _: &T) -> bool {
        self.visited_index != state.current_index as isize
    }
    fn increment_iteration_level(&mut self, state: &IterationState, _: &T) {
        self.visited_index = state.current_index as isize
    }
    fn has_next(&mut self, state: &IterationState, histogram: &T) -> bool {
        state.current_index < histogram.array_length() - 1
    }
}

pub struct RecordedValuesStrategy {
    pub(in crate::iteration) visited_index: isize,
}

impl<T: ReadableHistogram> IterationStrategy<T> for RecordedValuesStrategy {
    fn reached_iteration_level(&mut self, state: &IterationState, histogram: &T) -> bool {
        let current_count = histogram.unsafe_get_count_at_index(state.current_index);
        current_count != 0 && self.visited_index != state.current_index as isize
    }

    fn increment_iteration_level(&mut self, state: &IterationState, _: &T) {
        self.visited_index = state.current_index as isize;
    }
}

pub struct LinearStrategy {
    pub(in crate::iteration) value_units_per_bucket: u64,
    pub(in crate::iteration) current_step_highest_value_reporting_level: u64,
    pub(in crate::iteration) current_step_lowest_value_reporting_level: u64,
}

impl<T: ReadableHistogram> IterationStrategy<T> for LinearStrategy {
    fn reached_iteration_level(&mut self, state: &IterationState, histogram: &T) -> bool {
        state.current_value_at_index >= self.current_step_lowest_value_reporting_level
            || state.current_index >= (histogram.array_length() - 1)
    }
    fn increment_iteration_level(&mut self, _: &IterationState, histogram: &T) {
        self.current_step_highest_value_reporting_level += self.value_units_per_bucket;
        self.current_step_lowest_value_reporting_level = histogram
            .settings()
            .lowest_equivalent_value(self.current_step_highest_value_reporting_level);
    }
    fn has_next(&mut self, state: &IterationState, _: &T) -> bool {
        default_has_next(state) || (self.current_step_highest_value_reporting_level < state.next_value_at_index)
    }
    fn get_value_iterated_to(&mut self, _: &IterationState, _: &T) -> u64 {
        self.current_step_highest_value_reporting_level
    }
}

pub struct LogarithmicStrategy {
    pub(in crate::iteration) value_units_in_first_bucket: u64,
    pub(in crate::iteration) log_base: f64,
    pub(in crate::iteration) next_value_reporting_level: f64,
    pub(in crate::iteration) current_step_highest_value_reporting_level: u64,
    pub(in crate::iteration) current_step_lowest_value_reporting_level: u64,
}

impl<T: ReadableHistogram> IterationStrategy<T> for LogarithmicStrategy {
    fn reached_iteration_level(&mut self, state: &IterationState, histogram: &T) -> bool {
        state.current_value_at_index >= self.current_step_lowest_value_reporting_level
            || state.current_index >= histogram.array_length() - 1
    }
    fn increment_iteration_level(&mut self, _: &IterationState, histogram: &T) {
        self.next_value_reporting_level *= self.log_base;
        self.current_step_highest_value_reporting_level = self.next_value_reporting_level as u64 - 1;
        self.current_step_lowest_value_reporting_level = histogram
            .settings()
            .lowest_equivalent_value(self.current_step_highest_value_reporting_level);
    }
    fn has_next(&mut self, state: &IterationState, histogram: &T) -> bool {
        default_has_next(state)
            || histogram
                .settings()
                .lowest_equivalent_value(self.next_value_reporting_level as u64) < state.next_value_at_index
    }
    fn get_value_iterated_to(&mut self, _: &IterationState, _: &T) -> u64 {
        self.current_step_highest_value_reporting_level
    }
}

pub struct PercentileStrategy {
    pub(in crate::iteration) percentile_ticks_per_half_distance: isize,
    pub(in crate::iteration) percentile_level_to_iterate_to: f64,
    pub(in crate::iteration) percentile_level_to_iterate_from: f64,
    pub(in crate::iteration) reached_last_recorded_value: bool,
}

impl<T: ReadableHistogram> IterationStrategy<T> for PercentileStrategy {
    fn reached_iteration_level(&mut self, state: &IterationState, _: &T) -> bool {
        if state.count_at_this_value == 0 {
            return false;
        }
        let current_percentile = 100.0 * (state.total_count_to_current_index as f64 / state.array_total_count as f64);
        current_percentile >= self.percentile_level_to_iterate_to
    }

    fn increment_iteration_level(&mut self, _: &IterationState, _: &T) {
        self.percentile_level_to_iterate_from = self.percentile_level_to_iterate_to;
        let exp = (f64::ln(100.0 / (100.0 - self.percentile_level_to_iterate_to)) / f64::ln(2.0))
            as i64;
        let exp = exp.saturating_add(1);
        let exp = if exp < 0 {
            0_u32
        } else if exp > 62 {
            62_u32
        } else {
            exp as u32
        };
        let factor = 1_u64 << exp;
        let ticks_per_half = self.percentile_ticks_per_half_distance as u64;
        let percentile_reporting_ticks = ticks_per_half.saturating_mul(factor);
        if percentile_reporting_ticks == 0 {
            self.percentile_level_to_iterate_to = 100.0;
        } else {
            self.percentile_level_to_iterate_to += 100.0 / percentile_reporting_ticks as f64;
        }
    }

    fn has_next(&mut self, state: &IterationState, _: &T) -> bool {
        if default_has_next(state) {
            return true;
        }
        if !self.reached_last_recorded_value && state.array_total_count > 0 {
            self.percentile_level_to_iterate_to = 100.0;
            self.reached_last_recorded_value = true;
            true
        } else {
            false
        }
    }

    fn get_percentile_iterated_to(&mut self, _: &IterationState) -> f64 {
        self.percentile_level_to_iterate_to
    }
}
