use num;

pub trait Counter
    : num::Num + num::ToPrimitive + num::FromPrimitive + num::NumAssignOps + PartialOrd<Self> + Copy
    {
    /// Counter as a f64.
    fn as_f64(&self) -> f64;
    /// Counter as a u64.
    fn as_u64(&self) -> u64;
    fn word_size() -> u8;
}

impl Counter for u32 {
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
