/// Validate derived double range state at decode/shift boundaries, never on
/// the ordinary recording path. A finite positive ratio can still have an
/// infinite reciprocal, and finite inputs can overflow the published limits.
pub(crate) fn is_valid_range(lowest: f64, highest: f64, lowest_tracking_integer_value: u64) -> bool {
    let integer_to_double_ratio = lowest / lowest_tracking_integer_value as f64;
    lowest.is_finite()
        && lowest > 0.0
        && highest.is_finite()
        && highest > lowest
        && integer_to_double_ratio > 0.0
        && (1.0 / integer_to_double_ratio).is_finite()
}

/// Non-negative spacing at the magnitude of `value`, including subnormals and
/// f64::MAX. Like Java's Math.ulp, negative inputs have the same ULP as positive
/// inputs; stepping toward zero would give the wrong spacing at powers of two.
pub(crate) fn ulp(value: f64) -> f64 {
    let magnitude = value.abs();
    if !magnitude.is_finite() {
        return magnitude;
    }
    let bits = magnitude.to_bits();
    if magnitude == f64::MAX {
        magnitude - f64::from_bits(bits - 1)
    } else {
        f64::from_bits(bits + 1) - magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulp_is_positive_and_symmetric_at_float_boundaries() {
        let smallest = f64::from_bits(1);
        for (value, expected) in [
            (0.0, smallest),
            (smallest, smallest),
            (f64::MIN_POSITIVE, smallest),
            (0.5, f64::EPSILON / 2.0),
            (1.0, f64::EPSILON),
            (f64::MAX, f64::from_bits((971 + 1023) << 52)),
            (f64::INFINITY, f64::INFINITY),
        ] {
            assert_eq!(ulp(value), expected);
            assert_eq!(ulp(-value), expected);
        }
        assert!(ulp(f64::NAN).is_nan());
    }

    #[test]
    fn ranges_require_finite_bounds_and_reciprocal() {
        assert!(is_valid_range(1.0, 8.0, 1_024));
        assert!(is_valid_range(f64::MIN_POSITIVE, f64::MIN_POSITIVE * 8.0, 1));
        for (lowest, highest) in [
            (0.0, 1.0),
            (-1.0, 1.0),
            (1.0, 1.0),
            (1.0, f64::INFINITY),
            (f64::INFINITY, f64::INFINITY),
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::MIN_POSITIVE, f64::MIN_POSITIVE * 8.0),
            (f64::from_bits(1), f64::from_bits(8)),
        ] {
            assert!(!is_valid_range(lowest, highest, 1_024));
        }
    }
}
