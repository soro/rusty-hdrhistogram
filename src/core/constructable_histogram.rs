use crate::core::CreationError;

pub trait ConstructableHistogram: Sized {
    fn new(lowest_discernible_value: u64, highest_trackable_value: u64, significant_value_digits: u8) -> Result<Self, CreationError>;

    fn establish_internal_tracking_values(&mut self);
}
