use core::Counter;

pub trait SliceableHistogram<T: Counter> {
    fn get_counts_slice<'a>(&'a self, length: u32) -> Option<&'a [T]>;
    fn get_counts_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]>;
}

pub trait ReadSliceableHistogram<T: Counter> {
    fn get_counts_slice<'a>(&'a self, length: u32) -> Option<&'a [T]>;
}

pub trait MutSliceableHistogram<T: Counter> {
    fn get_counts_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]>;
}

impl<T: Counter, S> SliceableHistogram<T> for S
where
    S: ReadSliceableHistogram<T> + MutSliceableHistogram<T>,
{
    fn get_counts_slice<'a>(&'a self, length: u32) -> Option<&'a [T]> {
        <Self as ReadSliceableHistogram<T>>::get_counts_slice(self, length)
    }
    fn get_counts_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]> {
        <Self as MutSliceableHistogram<T>>::get_counts_slice_mut(self, length)
    }
}
