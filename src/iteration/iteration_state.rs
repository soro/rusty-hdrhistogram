use core::ReadableHistogram;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IterationState {
    pub array_total_count: u64,
    pub count_at_this_value: u64,
    pub current_index: u32,
    // TODO: maybe replace with highest_equivalent_value of current_value
    pub current_value_at_index: u64,
    pub fresh_sub_bucket: bool,
    pub next_value_at_index: u64,
    pub prev_value_iterated_to: u64,
    pub total_count_to_prev_index: u64,
    pub total_count_to_current_index: u64,
    pub total_value_to_current_index: u64,
    pub integer_to_double_value_conversion_ratio: f64,
}

impl IterationState {
    pub fn new<T: ReadableHistogram>(histogram: &T) -> IterationState {
        IterationState {
            array_total_count: histogram.get_total_count(),
            count_at_this_value: 0,
            current_index: 0,
            current_value_at_index: 0,
            fresh_sub_bucket: true,
            next_value_at_index: 1 << histogram.settings().unit_magnitude,
            prev_value_iterated_to: 0,
            total_count_to_prev_index: 0,
            total_count_to_current_index: 0,
            total_value_to_current_index: 0,
            integer_to_double_value_conversion_ratio: histogram
                .settings()
                .integer_to_double_value_conversion_ratio,
        }
    }

    pub fn reset<T: ReadableHistogram>(&mut self, histogram: &T) {
        self.array_total_count = histogram.get_total_count();
        self.count_at_this_value = 0;
        self.current_index = 0;
        self.current_value_at_index = 0;
        self.fresh_sub_bucket = true;
        self.next_value_at_index = 1 << histogram.settings().unit_magnitude;
        self.prev_value_iterated_to = 0;
        self.total_count_to_prev_index = 0;
        self.total_count_to_current_index = 0;
        self.total_value_to_current_index = 0;
        self.integer_to_double_value_conversion_ratio = histogram
            .settings()
            .integer_to_double_value_conversion_ratio;
    }
}
