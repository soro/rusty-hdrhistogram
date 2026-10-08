use std::ops::{AddAssign, SubAssign};

mod sealed {
    pub trait Sealed {}

    impl Sealed for u32 {}
    impl Sealed for u64 {}
}

/// A supported integer bucket counter: `u32` or `u64`.
///
/// This trait is sealed because histogram storage and unchecked indexing rely
/// on the built-in counters' zero initialization and arithmetic semantics.
/// Custom counter implementations are not supported.
///
/// ```compile_fail,E0277
/// use hdrhistogram::st::Counter;
/// #[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
/// struct Custom(u64);
/// # impl std::ops::AddAssign for Custom {
/// #     fn add_assign(&mut self, rhs: Self) { self.0 += rhs.0; }
/// # }
/// # impl std::ops::SubAssign for Custom {
/// #     fn sub_assign(&mut self, rhs: Self) { self.0 -= rhs.0; }
/// # }
/// impl Counter for Custom {
///     fn zero() -> Self { Custom(0) }
///     fn one() -> Self { Custom(1) }
///     fn as_f64(&self) -> f64 { self.0 as f64 }
///     fn as_u64(&self) -> u64 { self.0 }
///     fn word_size() -> u8 { 8 }
/// }
/// ```
pub trait Counter: sealed::Sealed + Copy + PartialOrd<Self> + AddAssign + SubAssign + Default {
    fn zero() -> Self;
    fn one() -> Self;
    /// Counter as a f64.
    fn as_f64(&self) -> f64;
    /// Counter as a u64.
    fn as_u64(&self) -> u64;
    fn word_size() -> u8;
}

impl Counter for u32 {
    #[inline(always)]
    fn zero() -> Self {
        0
    }
    #[inline(always)]
    fn one() -> Self {
        1
    }
    #[inline(always)]
    fn as_f64(&self) -> f64 {
        f64::from(*self)
    }
    #[inline(always)]
    fn as_u64(&self) -> u64 {
        u64::from(*self)
    }
    #[inline(always)]
    fn word_size() -> u8 {
        4
    }
}

impl Counter for u64 {
    #[inline(always)]
    fn zero() -> Self {
        0
    }
    #[inline(always)]
    fn one() -> Self {
        1
    }
    #[inline(always)]
    fn as_f64(&self) -> f64 {
        *self as f64
    }
    #[inline(always)]
    fn as_u64(&self) -> u64 {
        *self
    }
    #[inline(always)]
    fn word_size() -> u8 {
        8
    }
}
