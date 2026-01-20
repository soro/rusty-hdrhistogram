pub trait OverflowPolicy {
    const SATURATE: bool;
}

pub struct ThrowOnOverflow;

impl OverflowPolicy for ThrowOnOverflow {
    const SATURATE: bool = false;
}

pub struct SaturateOnOverflow;

impl OverflowPolicy for SaturateOnOverflow {
    const SATURATE: bool = true;
}
