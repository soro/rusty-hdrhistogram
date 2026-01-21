use crate::concurrent::ConcurrentDoubleHistogram;
use crate::core::{DoubleCreationError, RecordError};
use crate::st::DoubleHistogram;

const TRACKABLE_VALUE_RANGE_SIZE: u64 = 3600 * 1000 * 1000;
const NUMBER_OF_SIGNIFICANT_VALUE_DIGITS: u8 = 3;
const TEST_VALUE_LEVEL: f64 = 4.0;

trait TestDoubleHistogram: Sized {
    fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError>;
    fn with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio: u64,
        number_of_significant_value_digits: u8,
    ) -> Result<Self, DoubleCreationError>;
    fn record_value(&mut self, value: f64) -> Result<(), RecordError>;
    fn record_value_with_count(&mut self, value: f64, count: u64) -> Result<(), RecordError>;
    fn record_value_with_expected_interval(
        &mut self,
        value: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError>;
    fn get_count_at_value(&self, value: f64) -> u64;
    fn get_total_count(&self) -> u64;
    fn get_min_value(&self) -> f64;
    fn get_max_value(&self) -> f64;
    fn get_mean(&self) -> f64;
    fn get_std_deviation(&self) -> f64;
    fn get_value_at_percentile(&self, percentile: f64) -> f64;
    fn size_of_equivalent_value_range(&self, value: f64) -> f64;
    fn lowest_equivalent_value(&self, value: f64) -> f64;
    fn highest_equivalent_value(&self, value: f64) -> f64;
    fn median_equivalent_value(&self, value: f64) -> f64;
    fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool;
    fn get_current_lowest_trackable_non_zero_value(&self) -> f64;
    fn get_current_highest_trackable_value(&self) -> f64;
    fn get_highest_to_lowest_value_ratio(&self) -> u64;
    fn get_number_of_significant_value_digits(&self) -> u8;
    fn set_auto_resize(&mut self, auto_resize: bool);
    fn reset(&mut self);
    fn add(&mut self, other: &Self) -> Result<(), RecordError>;
    fn copy_corrected_for_coordinated_omission(
        &self,
        expected_interval_between_value_samples: f64,
    ) -> Result<Self, RecordError>;
}

impl TestDoubleHistogram for DoubleHistogram {
    fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError> {
        DoubleHistogram::new(number_of_significant_value_digits)
    }
    fn with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio: u64,
        number_of_significant_value_digits: u8,
    ) -> Result<Self, DoubleCreationError> {
        DoubleHistogram::with_highest_to_lowest_value_ratio(
            highest_to_lowest_value_ratio,
            number_of_significant_value_digits,
        )
    }
    fn record_value(&mut self, value: f64) -> Result<(), RecordError> {
        DoubleHistogram::record_value(self, value)
    }
    fn record_value_with_count(&mut self, value: f64, count: u64) -> Result<(), RecordError> {
        DoubleHistogram::record_value_with_count(self, value, count)
    }
    fn record_value_with_expected_interval(
        &mut self,
        value: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        DoubleHistogram::record_value_with_expected_interval(self, value, expected_interval_between_value_samples)
    }
    fn get_count_at_value(&self, value: f64) -> u64 {
        DoubleHistogram::get_count_at_value(self, value)
    }
    fn get_total_count(&self) -> u64 {
        DoubleHistogram::get_total_count(self)
    }
    fn get_min_value(&self) -> f64 {
        DoubleHistogram::get_min_value(self)
    }
    fn get_max_value(&self) -> f64 {
        DoubleHistogram::get_max_value(self)
    }
    fn get_mean(&self) -> f64 {
        DoubleHistogram::get_mean(self)
    }
    fn get_std_deviation(&self) -> f64 {
        DoubleHistogram::get_std_deviation(self)
    }
    fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        DoubleHistogram::get_value_at_percentile(self, percentile)
    }
    fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        DoubleHistogram::size_of_equivalent_value_range(self, value)
    }
    fn lowest_equivalent_value(&self, value: f64) -> f64 {
        DoubleHistogram::lowest_equivalent_value(self, value)
    }
    fn highest_equivalent_value(&self, value: f64) -> f64 {
        DoubleHistogram::highest_equivalent_value(self, value)
    }
    fn median_equivalent_value(&self, value: f64) -> f64 {
        DoubleHistogram::median_equivalent_value(self, value)
    }
    fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool {
        DoubleHistogram::values_are_equivalent(self, value1, value2)
    }
    fn get_current_lowest_trackable_non_zero_value(&self) -> f64 {
        DoubleHistogram::get_current_lowest_trackable_non_zero_value(self)
    }
    fn get_current_highest_trackable_value(&self) -> f64 {
        DoubleHistogram::get_current_highest_trackable_value(self)
    }
    fn get_highest_to_lowest_value_ratio(&self) -> u64 {
        DoubleHistogram::get_highest_to_lowest_value_ratio(self)
    }
    fn get_number_of_significant_value_digits(&self) -> u8 {
        DoubleHistogram::get_number_of_significant_value_digits(self)
    }
    fn set_auto_resize(&mut self, auto_resize: bool) {
        DoubleHistogram::set_auto_resize(self, auto_resize)
    }
    fn reset(&mut self) {
        DoubleHistogram::reset(self)
    }
    fn add(&mut self, other: &Self) -> Result<(), RecordError> {
        DoubleHistogram::add(self, other)
    }
    fn copy_corrected_for_coordinated_omission(
        &self,
        expected_interval_between_value_samples: f64,
    ) -> Result<Self, RecordError> {
        DoubleHistogram::copy_corrected_for_coordinated_omission(self, expected_interval_between_value_samples)
    }
}

impl TestDoubleHistogram for ConcurrentDoubleHistogram {
    fn new(number_of_significant_value_digits: u8) -> Result<Self, DoubleCreationError> {
        ConcurrentDoubleHistogram::new(number_of_significant_value_digits)
    }
    fn with_highest_to_lowest_value_ratio(
        highest_to_lowest_value_ratio: u64,
        number_of_significant_value_digits: u8,
    ) -> Result<Self, DoubleCreationError> {
        ConcurrentDoubleHistogram::with_highest_to_lowest_value_ratio(
            highest_to_lowest_value_ratio,
            number_of_significant_value_digits,
        )
    }
    fn record_value(&mut self, value: f64) -> Result<(), RecordError> {
        ConcurrentDoubleHistogram::record_value(self, value)
    }
    fn record_value_with_count(&mut self, value: f64, count: u64) -> Result<(), RecordError> {
        ConcurrentDoubleHistogram::record_value_with_count(self, value, count)
    }
    fn record_value_with_expected_interval(
        &mut self,
        value: f64,
        expected_interval_between_value_samples: f64,
    ) -> Result<(), RecordError> {
        ConcurrentDoubleHistogram::record_value_with_expected_interval(
            self,
            value,
            expected_interval_between_value_samples,
        )
    }
    fn get_count_at_value(&self, value: f64) -> u64 {
        ConcurrentDoubleHistogram::get_count_at_value(self, value)
    }
    fn get_total_count(&self) -> u64 {
        ConcurrentDoubleHistogram::get_total_count(self)
    }
    fn get_min_value(&self) -> f64 {
        ConcurrentDoubleHistogram::get_min_value(self)
    }
    fn get_max_value(&self) -> f64 {
        ConcurrentDoubleHistogram::get_max_value(self)
    }
    fn get_mean(&self) -> f64 {
        ConcurrentDoubleHistogram::get_mean(self)
    }
    fn get_std_deviation(&self) -> f64 {
        ConcurrentDoubleHistogram::get_std_deviation(self)
    }
    fn get_value_at_percentile(&self, percentile: f64) -> f64 {
        ConcurrentDoubleHistogram::get_value_at_percentile(self, percentile)
    }
    fn size_of_equivalent_value_range(&self, value: f64) -> f64 {
        ConcurrentDoubleHistogram::size_of_equivalent_value_range(self, value)
    }
    fn lowest_equivalent_value(&self, value: f64) -> f64 {
        ConcurrentDoubleHistogram::lowest_equivalent_value(self, value)
    }
    fn highest_equivalent_value(&self, value: f64) -> f64 {
        ConcurrentDoubleHistogram::highest_equivalent_value(self, value)
    }
    fn median_equivalent_value(&self, value: f64) -> f64 {
        ConcurrentDoubleHistogram::median_equivalent_value(self, value)
    }
    fn values_are_equivalent(&self, value1: f64, value2: f64) -> bool {
        ConcurrentDoubleHistogram::values_are_equivalent(self, value1, value2)
    }
    fn get_current_lowest_trackable_non_zero_value(&self) -> f64 {
        ConcurrentDoubleHistogram::get_current_lowest_trackable_non_zero_value(self)
    }
    fn get_current_highest_trackable_value(&self) -> f64 {
        ConcurrentDoubleHistogram::get_current_highest_trackable_value(self)
    }
    fn get_highest_to_lowest_value_ratio(&self) -> u64 {
        ConcurrentDoubleHistogram::get_highest_to_lowest_value_ratio(self)
    }
    fn get_number_of_significant_value_digits(&self) -> u8 {
        ConcurrentDoubleHistogram::get_number_of_significant_value_digits(self)
    }
    fn set_auto_resize(&mut self, auto_resize: bool) {
        ConcurrentDoubleHistogram::set_auto_resize(self, auto_resize)
    }
    fn reset(&mut self) {
        ConcurrentDoubleHistogram::reset(self)
    }
    fn add(&mut self, other: &Self) -> Result<(), RecordError> {
        ConcurrentDoubleHistogram::add(self, other)
    }
    fn copy_corrected_for_coordinated_omission(
        &self,
        expected_interval_between_value_samples: f64,
    ) -> Result<Self, RecordError> {
        ConcurrentDoubleHistogram::copy_corrected_for_coordinated_omission(
            self,
            expected_interval_between_value_samples,
        )
    }
}

fn find_containing_binary_order_of_magnitude(long_number: u64) -> u32 {
    let pow2_ceiling = 64 - long_number.leading_zeros();
    std::cmp::min(pow2_ceiling, 62)
}

fn run_range_ratio_must_be_ge_two_test<H: TestDoubleHistogram>() {
    assert!(H::with_highest_to_lowest_value_ratio(1, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).is_err());
}

#[test]
fn trackable_value_range_must_be_greater_than_two() {
    run_range_ratio_must_be_ge_two_test::<DoubleHistogram>();
    run_range_ratio_must_be_ge_two_test::<ConcurrentDoubleHistogram>();
}

fn run_sig_digits_must_be_le_five_test<H: TestDoubleHistogram>() {
    assert!(H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, 6).is_err());
}

#[test]
fn number_of_significant_value_digits_must_be_less_than_six() {
    run_sig_digits_must_be_le_five_test::<DoubleHistogram>();
    run_sig_digits_must_be_le_five_test::<ConcurrentDoubleHistogram>();
}

fn run_construction_argument_gets_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(f64::powi(2.0, 20)));
    succ!(histogram.record_value(1.0));
    assert_approx_eq!(1.0, histogram.get_current_lowest_trackable_non_zero_value(), 0.001);
    assert_eq!(TRACKABLE_VALUE_RANGE_SIZE, histogram.get_highest_to_lowest_value_ratio());
    assert_eq!(NUMBER_OF_SIGNIFICANT_VALUE_DIGITS, histogram.get_number_of_significant_value_digits());

    let mut histogram2 =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram2.record_value(2048.0 * 1024.0 * 1024.0));
    assert_approx_eq!(
        2048.0 * 1024.0 * 1024.0,
        histogram2.get_current_lowest_trackable_non_zero_value(),
        0.001
    );

    let mut histogram3 =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram3.record_value(1.0 / 1000.0));
    assert_approx_eq!(
        1.0 / 1024.0,
        histogram3.get_current_lowest_trackable_non_zero_value(),
        0.001
    );
}

#[test]
fn construction_argument_gets() {
    run_construction_argument_gets_test::<DoubleHistogram>();
    run_construction_argument_gets_test::<ConcurrentDoubleHistogram>();
}

fn run_data_range_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(0.0));
    assert_eq!(1, histogram.get_count_at_value(0.0));

    let mut top_value = 1.0;
    loop {
        let res = histogram.record_value(top_value);
        if res.is_err() {
            break;
        }
        top_value *= 2.0;
    }
    assert_approx_eq!((1_u64 << 33) as f64, top_value, 0.00001);
    assert_eq!(1, histogram.get_count_at_value(0.0));

    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(0.0));

    let mut bottom_value = (1_u64 << 33) as f64;
    loop {
        let res = histogram.record_value(bottom_value);
        if res.is_err() {
            break;
        }
        bottom_value /= 2.0;
    }
    assert_approx_eq!(1.0, bottom_value, 0.00001);

    let expected_range = 1_u64 << (find_containing_binary_order_of_magnitude(TRACKABLE_VALUE_RANGE_SIZE) + 1);
    assert_approx_eq!(expected_range as f64, top_value / bottom_value, 0.00001);
    assert_eq!(1, histogram.get_count_at_value(0.0));
}

#[test]
fn data_range() {
    run_data_range_test::<DoubleHistogram>();
    run_data_range_test::<ConcurrentDoubleHistogram>();
}

fn run_record_value_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    assert_eq!(1, histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(1, histogram.get_total_count());
}

#[test]
fn record_value() {
    run_record_value_test::<DoubleHistogram>();
    run_record_value_test::<ConcurrentDoubleHistogram>();
}

fn run_record_value_overflow_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    assert!(histogram
        .record_value(TRACKABLE_VALUE_RANGE_SIZE as f64 * 3.0)
        .is_err());
    succ!(histogram.record_value(1.0));
}

#[test]
fn record_value_overflow_should_throw() {
    run_record_value_overflow_test::<DoubleHistogram>();
    run_record_value_overflow_test::<ConcurrentDoubleHistogram>();
}

fn run_record_value_with_expected_interval_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(0.0));
    succ!(histogram.record_value_with_expected_interval(TEST_VALUE_LEVEL, TEST_VALUE_LEVEL / 4.0));

    let mut raw_histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(raw_histogram.record_value(0.0));
    succ!(raw_histogram.record_value(TEST_VALUE_LEVEL));

    assert_eq!(1, raw_histogram.get_count_at_value(0.0));
    assert_eq!(0, raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 1.0) / 4.0));
    assert_eq!(0, raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 2.0) / 4.0));
    assert_eq!(0, raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 3.0) / 4.0));
    assert_eq!(1, raw_histogram.get_count_at_value((TEST_VALUE_LEVEL * 4.0) / 4.0));
    assert_eq!(2, raw_histogram.get_total_count());

    assert_eq!(1, histogram.get_count_at_value(0.0));
    assert_eq!(1, histogram.get_count_at_value((TEST_VALUE_LEVEL * 1.0) / 4.0));
    assert_eq!(1, histogram.get_count_at_value((TEST_VALUE_LEVEL * 2.0) / 4.0));
    assert_eq!(1, histogram.get_count_at_value((TEST_VALUE_LEVEL * 3.0) / 4.0));
    assert_eq!(1, histogram.get_count_at_value((TEST_VALUE_LEVEL * 4.0) / 4.0));
    assert_eq!(5, histogram.get_total_count());
}

#[test]
fn record_value_with_expected_interval() {
    run_record_value_with_expected_interval_test::<DoubleHistogram>();
    run_record_value_with_expected_interval_test::<ConcurrentDoubleHistogram>();
}

fn run_reset_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(10.0));
    succ!(histogram.record_value(100.0));
    assert_approx_eq!(histogram.get_min_value(), f64::min(10.0, TEST_VALUE_LEVEL), 1.0);
    assert_approx_eq!(histogram.get_max_value(), f64::max(100.0, TEST_VALUE_LEVEL), 1.0);
    histogram.reset();
    assert_eq!(0, histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(0, histogram.get_total_count());
    succ!(histogram.record_value(20.0));
    succ!(histogram.record_value(80.0));
    assert_approx_eq!(20.0, histogram.get_min_value(), 1.0);
    assert_approx_eq!(80.0, histogram.get_max_value(), 1.0);
}

#[test]
fn reset() {
    run_reset_test::<DoubleHistogram>();
    run_reset_test::<ConcurrentDoubleHistogram>();
}

fn run_add_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    let mut other =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();

    succ!(histogram.record_value(TEST_VALUE_LEVEL));
    succ!(histogram.record_value(TEST_VALUE_LEVEL * 1000.0));
    succ!(other.record_value(TEST_VALUE_LEVEL));
    succ!(other.record_value(TEST_VALUE_LEVEL * 1000.0));
    succ!(histogram.add(&other));
    assert_eq!(2, histogram.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(2, histogram.get_count_at_value(TEST_VALUE_LEVEL * 1000.0));
    assert_eq!(4, histogram.get_total_count());

    let mut bigger_other =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE * 2, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS)
            .unwrap();
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL));
    succ!(bigger_other.record_value(TEST_VALUE_LEVEL * 1000.0));

    succ!(bigger_other.add(&histogram));
    assert_eq!(3, bigger_other.get_count_at_value(TEST_VALUE_LEVEL));
    assert_eq!(3, bigger_other.get_count_at_value(TEST_VALUE_LEVEL * 1000.0));
    assert_eq!(6, bigger_other.get_total_count());

    succ!(histogram.add(&bigger_other));

    succ!(histogram.record_value(1.0));
    succ!(other.record_value(1.0));
    succ!(bigger_other.record_value(TRACKABLE_VALUE_RANGE_SIZE as f64 * 8.0));
    assert!(bigger_other.add(&histogram).is_err());
}

#[test]
fn add() {
    run_add_test::<DoubleHistogram>();
    run_add_test::<ConcurrentDoubleHistogram>();
}

fn run_add_with_auto_resize_test<H: TestDoubleHistogram>() {
    let mut histo1 = H::new(3).unwrap();
    histo1.set_auto_resize(true);
    succ!(histo1.record_value(6.0));
    succ!(histo1.record_value(1.0));
    succ!(histo1.record_value(5.0));
    succ!(histo1.record_value(8.0));
    succ!(histo1.record_value(3.0));
    succ!(histo1.record_value(7.0));
    let mut histo2 = H::new(3).unwrap();
    histo2.set_auto_resize(true);
    succ!(histo2.record_value(9.0));
    let mut histo3 = H::new(3).unwrap();
    histo3.set_auto_resize(true);
    succ!(histo3.record_value(4.0));
    succ!(histo3.record_value(2.0));
    succ!(histo3.record_value(10.0));

    let mut merged = H::new(3).unwrap();
    merged.set_auto_resize(true);
    succ!(merged.add(&histo1));
    succ!(merged.add(&histo2));
    succ!(merged.add(&histo3));

    assert_eq!(
        merged.get_total_count(),
        histo1.get_total_count() + histo2.get_total_count() + histo3.get_total_count()
    );
    assert_approx_eq!(1.0, merged.get_min_value(), 0.01);
    assert_approx_eq!(10.0, merged.get_max_value(), 0.01);
}

#[test]
fn add_with_auto_resize() {
    run_add_with_auto_resize_test::<DoubleHistogram>();
    run_add_with_auto_resize_test::<ConcurrentDoubleHistogram>();
}

fn run_equivalent_value_range_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(1.0));
    assert_approx_eq!(1.0 / 1024.0, histogram.size_of_equivalent_value_range(1.0), 0.001);
    assert_approx_eq!(2.0, histogram.size_of_equivalent_value_range(2500.0), 0.001);
    assert_approx_eq!(4.0, histogram.size_of_equivalent_value_range(8191.0), 0.001);
    assert_approx_eq!(8.0, histogram.size_of_equivalent_value_range(8192.0), 0.001);
    assert_approx_eq!(8.0, histogram.size_of_equivalent_value_range(10000.0), 0.001);
}

#[test]
fn size_of_equivalent_value_range() {
    run_equivalent_value_range_test::<DoubleHistogram>();
    run_equivalent_value_range_test::<ConcurrentDoubleHistogram>();
}

fn run_lowest_equivalent_value_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(1.0));
    assert_approx_eq!(10000.0, histogram.lowest_equivalent_value(10007.0), 0.001);
    assert_approx_eq!(10008.0, histogram.lowest_equivalent_value(10009.0), 0.001);
}

#[test]
fn lowest_equivalent_value() {
    run_lowest_equivalent_value_test::<DoubleHistogram>();
    run_lowest_equivalent_value_test::<ConcurrentDoubleHistogram>();
}

fn run_highest_equivalent_value_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(1.0));
    assert_approx_eq!(8183.99999, histogram.highest_equivalent_value(8180.0), 0.001);
    assert_approx_eq!(8191.99999, histogram.highest_equivalent_value(8191.0), 0.001);
    assert_approx_eq!(8199.99999, histogram.highest_equivalent_value(8193.0), 0.001);
    assert_approx_eq!(9999.99999, histogram.highest_equivalent_value(9995.0), 0.001);
    assert_approx_eq!(10007.99999, histogram.highest_equivalent_value(10007.0), 0.001);
    assert_approx_eq!(10015.99999, histogram.highest_equivalent_value(10008.0), 0.001);
}

#[test]
fn highest_equivalent_value() {
    run_highest_equivalent_value_test::<DoubleHistogram>();
    run_highest_equivalent_value_test::<ConcurrentDoubleHistogram>();
}

fn run_median_equivalent_value_test<H: TestDoubleHistogram>() {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    succ!(histogram.record_value(1.0));
    assert_approx_eq!(4.002, histogram.median_equivalent_value(4.0), 0.001);
    assert_approx_eq!(5.002, histogram.median_equivalent_value(5.0), 0.001);
    assert_approx_eq!(4001.0, histogram.median_equivalent_value(4000.0), 0.001);
    assert_approx_eq!(8002.0, histogram.median_equivalent_value(8000.0), 0.001);
    assert_approx_eq!(10004.0, histogram.median_equivalent_value(10007.0), 0.001);
}

#[test]
fn median_equivalent_value() {
    run_median_equivalent_value_test::<DoubleHistogram>();
    run_median_equivalent_value_test::<ConcurrentDoubleHistogram>();
}

fn run_max_value_test<H: TestDoubleHistogram>() {
    let mut histogram = H::with_highest_to_lowest_value_ratio(1_000_000_000, 2).unwrap();
    succ!(histogram.record_value(2.5362386543));
    let max_value = histogram.get_max_value();
    assert_approx_eq!(max_value, histogram.highest_equivalent_value(2.5362386543), 0.00001);
}

#[test]
fn max_value() {
    run_max_value_test::<DoubleHistogram>();
    run_max_value_test::<ConcurrentDoubleHistogram>();
}

struct DataHistograms<H: TestDoubleHistogram> {
    histogram: H,
    scaled_histogram: H,
    raw_histogram: H,
    scaled_raw_histogram: H,
    post_corrected_histogram: H,
    post_corrected_scaled_histogram: H,
}

fn build_data_histograms<H: TestDoubleHistogram>() -> DataHistograms<H> {
    let mut histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    let mut scaled_histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE / 2, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS)
            .unwrap();
    let mut raw_histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS).unwrap();
    let mut scaled_raw_histogram =
        H::with_highest_to_lowest_value_ratio(TRACKABLE_VALUE_RANGE_SIZE / 2, NUMBER_OF_SIGNIFICANT_VALUE_DIGITS)
            .unwrap();

    for _ in 0..10000 {
        succ!(histogram.record_value_with_expected_interval(1000.0, 10000.0));
        succ!(scaled_histogram.record_value_with_expected_interval(1000.0 * 512.0, 10000.0 * 512.0));
        succ!(raw_histogram.record_value(1000.0));
        succ!(scaled_raw_histogram.record_value(1000.0 * 512.0));
    }
    succ!(histogram.record_value_with_expected_interval(100000000.0, 10000.0));
    succ!(scaled_histogram.record_value_with_expected_interval(100000000.0 * 512.0, 10000.0 * 512.0));
    succ!(raw_histogram.record_value(100000000.0));
    succ!(scaled_raw_histogram.record_value(100000000.0 * 512.0));

    let post_corrected_histogram =
        raw_histogram.copy_corrected_for_coordinated_omission(10000.0).unwrap();
    let post_corrected_scaled_histogram =
        scaled_raw_histogram.copy_corrected_for_coordinated_omission(10000.0 * 512.0).unwrap();

    DataHistograms {
        histogram,
        scaled_histogram,
        raw_histogram,
        scaled_raw_histogram,
        post_corrected_histogram,
        post_corrected_scaled_histogram,
    }
}

fn run_scaling_equivalence_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();

    assert_approx_eq!(data.histogram.get_mean() * 512.0, data.scaled_histogram.get_mean(), data.scaled_histogram.get_mean() * 0.000001);
    assert_eq!(data.histogram.get_total_count(), data.scaled_histogram.get_total_count());
    assert_approx_eq!(
        data.scaled_histogram.highest_equivalent_value(data.histogram.get_value_at_percentile(99.0) * 512.0),
        data.scaled_histogram.highest_equivalent_value(data.scaled_histogram.get_value_at_percentile(99.0)),
        data.scaled_histogram.highest_equivalent_value(data.scaled_histogram.get_value_at_percentile(99.0)) * 0.000001
    );
    assert_approx_eq!(
        data.scaled_histogram.highest_equivalent_value(data.histogram.get_max_value() * 512.0),
        data.scaled_histogram.get_max_value(),
        data.scaled_histogram.get_max_value() * 0.000001
    );

    assert_approx_eq!(data.histogram.get_mean() * 512.0, data.scaled_histogram.get_mean(), data.scaled_histogram.get_mean() * 0.000001);
    assert_eq!(data.post_corrected_histogram.get_total_count(), data.post_corrected_scaled_histogram.get_total_count());
    assert_approx_eq!(
        data.post_corrected_histogram.lowest_equivalent_value(data.post_corrected_histogram.get_value_at_percentile(99.0)) * 512.0,
        data.post_corrected_scaled_histogram.lowest_equivalent_value(data.post_corrected_scaled_histogram.get_value_at_percentile(99.0)),
        data.post_corrected_scaled_histogram.lowest_equivalent_value(data.post_corrected_scaled_histogram.get_value_at_percentile(99.0)) * 0.000001
    );
    assert_approx_eq!(
        data.post_corrected_scaled_histogram.highest_equivalent_value(data.post_corrected_histogram.get_max_value() * 512.0),
        data.post_corrected_scaled_histogram.get_max_value(),
        data.post_corrected_scaled_histogram.get_max_value() * 0.000001
    );
}

#[test]
fn scaling_equivalence() {
    run_scaling_equivalence_test::<DoubleHistogram>();
    run_scaling_equivalence_test::<ConcurrentDoubleHistogram>();
}

fn run_get_total_count_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    assert_eq!(10001, data.raw_histogram.get_total_count());
    assert_eq!(20000, data.histogram.get_total_count());
}

#[test]
fn get_total_count() {
    run_get_total_count_test::<DoubleHistogram>();
    run_get_total_count_test::<ConcurrentDoubleHistogram>();
}

fn run_get_max_value_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    assert!(data.histogram.values_are_equivalent(100.0 * 1000.0 * 1000.0, data.histogram.get_max_value()));
}

#[test]
fn get_max_value() {
    run_get_max_value_test::<DoubleHistogram>();
    run_get_max_value_test::<ConcurrentDoubleHistogram>();
}

fn run_get_min_value_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    assert!(data.histogram.values_are_equivalent(1000.0, data.histogram.get_min_value()));
}

#[test]
fn get_min_value() {
    run_get_min_value_test::<DoubleHistogram>();
    run_get_min_value_test::<ConcurrentDoubleHistogram>();
}

fn run_get_mean_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    let expected_raw_mean = ((10000.0 * 1000.0) + (1.0 * 100000000.0)) / 10001.0;
    let expected_mean = (1000.0 + 50000000.0) / 2.0;
    assert_approx_eq!(expected_raw_mean, data.raw_histogram.get_mean(), expected_raw_mean * 0.001);
    assert_approx_eq!(expected_mean, data.histogram.get_mean(), expected_mean * 0.001);
}

#[test]
fn get_mean() {
    run_get_mean_test::<DoubleHistogram>();
    run_get_mean_test::<ConcurrentDoubleHistogram>();
}

fn run_get_std_deviation_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    let expected_raw_mean = ((10000.0 * 1000.0) + (1.0 * 100000000.0)) / 10001.0;
    let expected_raw_stddev = (((10000.0 * (1000.0 - expected_raw_mean).powi(2))
        + (100000000.0 - expected_raw_mean).powi(2))
        / 10001.0)
        .sqrt();

    let expected_mean = (1000.0 + 50000000.0) / 2.0;
    let mut expected_square_deviation_sum = 10000.0 * (1000.0 - expected_mean).powi(2);
    let mut value = 10000.0;
    while value <= 100000000.0 {
        expected_square_deviation_sum += (value - expected_mean).powi(2);
        value += 10000.0;
    }
    let expected_stddev = (expected_square_deviation_sum / 20000.0).sqrt();

    assert_approx_eq!(expected_raw_stddev, data.raw_histogram.get_std_deviation(), expected_raw_stddev * 0.001);
    assert_approx_eq!(expected_stddev, data.histogram.get_std_deviation(), expected_stddev * 0.001);
}

#[test]
fn get_std_deviation() {
    run_get_std_deviation_test::<DoubleHistogram>();
    run_get_std_deviation_test::<ConcurrentDoubleHistogram>();
}

fn run_get_value_at_percentile_test<H: TestDoubleHistogram>() {
    let data = build_data_histograms::<H>();
    assert_approx_eq!(1000.0, data.raw_histogram.get_value_at_percentile(30.0), 1000.0 * 0.001);
    assert_approx_eq!(1000.0, data.raw_histogram.get_value_at_percentile(99.0), 1000.0 * 0.001);
    assert_approx_eq!(1000.0, data.raw_histogram.get_value_at_percentile(99.99), 1000.0 * 0.001);
    assert_approx_eq!(100000000.0, data.raw_histogram.get_value_at_percentile(99.999), 100000000.0 * 0.001);
    assert_approx_eq!(100000000.0, data.raw_histogram.get_value_at_percentile(100.0), 100000000.0 * 0.001);

    assert_approx_eq!(1000.0, data.histogram.get_value_at_percentile(30.0), 1000.0 * 0.001);
    assert_approx_eq!(1000.0, data.histogram.get_value_at_percentile(50.0), 1000.0 * 0.001);
    assert_approx_eq!(50000000.0, data.histogram.get_value_at_percentile(75.0), 50000000.0 * 0.001);
    assert_approx_eq!(80000000.0, data.histogram.get_value_at_percentile(90.0), 80000000.0 * 0.001);
    assert_approx_eq!(98000000.0, data.histogram.get_value_at_percentile(99.0), 98000000.0 * 0.001);
    assert_approx_eq!(100000000.0, data.histogram.get_value_at_percentile(99.999), 100000000.0 * 0.001);
    assert_approx_eq!(100000000.0, data.histogram.get_value_at_percentile(100.0), 100000000.0 * 0.001);
}

#[test]
fn get_value_at_percentile() {
    run_get_value_at_percentile_test::<DoubleHistogram>();
    run_get_value_at_percentile_test::<ConcurrentDoubleHistogram>();
}
