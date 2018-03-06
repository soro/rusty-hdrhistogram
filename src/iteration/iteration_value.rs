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
