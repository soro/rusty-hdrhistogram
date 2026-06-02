#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IterationValue {
    pub value_iterated_to: u64,
    pub value_iterated_from: u64,
    pub count_at_value_iterated_to: u64,
    pub count_added_in_this_iteration_step: u64,
    pub total_count_to_this_value: u64,
    pub total_value_to_this_value: u64,
    pub percentile: f64,
    pub percentile_level_iterated_to: f64,
    pub integer_to_double_value_conversion_ratio: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoubleIterationValue {
    pub value_iterated_to: f64,
    pub value_iterated_from: f64,
    pub count_at_value_iterated_to: u64,
    pub count_added_in_this_iteration_step: u64,
    pub total_count_to_this_value: u64,
    pub total_value_to_this_value: f64,
    pub percentile: f64,
    pub percentile_level_iterated_to: f64,
    pub integer_iteration_value: IterationValue,
}

impl From<IterationValue> for DoubleIterationValue {
    fn from(value: IterationValue) -> Self {
        let ratio = value.integer_to_double_value_conversion_ratio;
        DoubleIterationValue {
            value_iterated_to: value.value_iterated_to as f64 * ratio,
            value_iterated_from: value.value_iterated_from as f64 * ratio,
            count_at_value_iterated_to: value.count_at_value_iterated_to,
            count_added_in_this_iteration_step: value.count_added_in_this_iteration_step,
            total_count_to_this_value: value.total_count_to_this_value,
            total_value_to_this_value: value.total_value_to_this_value as f64 * ratio,
            percentile: value.percentile,
            percentile_level_iterated_to: value.percentile_level_iterated_to,
            integer_iteration_value: value,
        }
    }
}
