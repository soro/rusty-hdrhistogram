use crate::core::{HistogramSettings, ReadableHistogram};
use crate::iteration::iteration_error::IterationError;
use crate::iteration::iteration_state::IterationState;
use crate::iteration::iteration_strategy::IterationStrategy;
use crate::iteration::IterationValue;

pub struct HistogramIterator<H, S> {
    pub(in crate::iteration) histogram: H,
    pub(in crate::iteration) state: IterationState,
    pub(in crate::iteration) strategy: S,
}

impl<H: ReadableHistogram, S: IterationStrategy<H>> HistogramIterator<H, S> {
    pub fn try_next_value(&mut self) -> Result<Option<IterationValue>, IterationError> {
        let state = &mut self.state;
        let strategy = &mut self.strategy;
        let histogram = &self.histogram;
        let settings = histogram.settings();
        Self::check_concurrent_modification(state, histogram)?;
        if strategy.has_next(state, histogram) {
            while state.current_index < histogram.array_length() {
                state.count_at_this_value = histogram.unsafe_get_count_at_index(state.current_index);
                if state.fresh_sub_bucket {
                    state.total_count_to_current_index += state.count_at_this_value;
                    state.total_value_to_current_index +=
                        state.count_at_this_value * settings.highest_equivalent_value(state.current_value_at_index);
                    state.fresh_sub_bucket = false
                }
                Self::check_concurrent_modification(state, histogram)?;
                if strategy.reached_iteration_level(state, histogram) {
                    let value_iterated_to = strategy.get_value_iterated_to(state, histogram);
                    let iteration_value = IterationValue {
                        value_iterated_to,
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
                    Self::check_concurrent_modification(state, histogram)?;

                    return Ok(Some(iteration_value));
                }
                Self::increment_sub_bucket(state, &settings);
            }
            Self::check_concurrent_modification(state, histogram)?;
            Ok(None)
        } else {
            Ok(None)
        }
    }

    pub fn next_value(&mut self) -> Option<IterationValue> {
        self.try_next_value()
            .expect("IterableHistogram sources must not be concurrently modified")
    }

    pub fn check_not_concurrently_modified(&self) -> Result<(), IterationError> {
        Self::check_concurrent_modification(&self.state, &self.histogram)
    }

    fn check_concurrent_modification(state: &IterationState, histogram: &H) -> Result<(), IterationError> {
        if histogram.current_total_count() != state.array_total_count || state.total_count_to_current_index > state.array_total_count {
            Err(IterationError::ConcurrentModification)
        } else {
            Ok(())
        }
    }

    fn increment_sub_bucket(state: &mut IterationState, settings: &HistogramSettings) {
        state.fresh_sub_bucket = true;
        state.current_index += 1;
        state.current_value_at_index = settings.value_from_index(state.current_index);
        state.next_value_at_index = settings.value_from_index(state.current_index + 1);
    }
}
