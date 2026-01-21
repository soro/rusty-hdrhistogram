use std::ops::{AddAssign, SubAssign};

pub trait Counter: Copy + PartialOrd<Self> + AddAssign + SubAssign + Default {
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
