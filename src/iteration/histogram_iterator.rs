use core::{HistogramSettings, ReadableHistogram};
use iteration::*;

pub struct HistogramIterator<'a, T: 'a, S> {
    pub(in iteration) histogram: &'a T,
    pub(in iteration) state: IterationState,
    pub(in iteration) strategy: S,
}

impl<'a, T: ReadableHistogram, S: IterationStrategy<T>> HistogramIterator<'a, T, S> {
    pub fn next_value(&mut self) -> Option<IterationValue> {
        let state = &mut self.state;
        let strategy = &mut self.strategy;
        let histogram = &*self.histogram;
        let settings = self.histogram.settings();
        if strategy.has_next(state, histogram) {
            while !(state.current_index >= histogram.array_length()) {
                state.count_at_this_value = histogram.unsafe_get_count_at_index(state.current_index);
                if state.fresh_sub_bucket {
                    state.total_count_to_current_index += state.count_at_this_value;
                    state.total_value_to_current_index +=
                        state.count_at_this_value * settings.highest_equivalent_value(state.current_value_at_index);
                    state.fresh_sub_bucket = false
                }
                if strategy.reached_iteration_level(state, histogram) {
                    let value_iterated_to = strategy.get_value_iterated_to(state, histogram);
                    let iteration_value = IterationValue {
                        value_iterated_to: value_iterated_to,
                        count_at_value_iterated_to: state.count_at_this_value,
                        value_iterated_from: state.prev_value_iterated_to,
                        count_added_in_this_iteration_step: state.total_count_to_current_index - state.total_count_to_prev_index,
                        total_count_to_this_value: state.total_count_to_current_index,
                        total_value_to_this_value: state.total_value_to_current_index,
                        percentile: (100.0 * state.total_count_to_current_index as f64) / state.array_total_count as f64,
                        percentile_level_iterated_to: strategy.get_percentile_iterated_to(state),
                        integer_to_double_value_conversion_ratio: state.integer_to_double_value_conversion_ratio,
                    };

                    state.prev_value_iterated_to = value_iterated_to;
                    state.total_count_to_prev_index = state.total_count_to_current_index;
                    strategy.increment_iteration_level(state, histogram);

                    return Some(iteration_value);
                }
                Self::increment_sub_bucket(state, settings);
            }
            panic!("should not get here - iteration level logic is faulty or histogram was modified concurrently")
        } else {
            None
        }
    }

    fn increment_sub_bucket(state: &mut IterationState, settings: &HistogramSettings) {
        state.fresh_sub_bucket = true;
        state.current_index += 1;
        state.current_value_at_index = settings.value_from_index(state.current_index);
        state.next_value_at_index = settings.value_from_index(state.current_index + 1);
    }
}
