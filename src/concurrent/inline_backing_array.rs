use crate::core::{util, HistogramStorageMetadata};
use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::marker::PhantomData;
use std::mem;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

#[repr(C)]
pub(crate) struct InlineBackingArray<T> {
    length: u32,
    bucket_count: u32,
    highest_trackable_value: u64,
    normalizing_index_offset: AtomicI32,
    double_to_integer_value_conversion_ratio: AtomicU64,
    _marker: PhantomData<T>,
}

impl<T> InlineBackingArray<T> {
    #[inline(always)]
    fn array_offset() -> usize {
        let header_size = mem::size_of::<InlineBackingArray<T>>();
        let array_align = mem::align_of::<T>();
        (header_size + array_align - 1) & !(array_align - 1)
    }

    fn layout(length: u32) -> (Layout, usize) {
        let header = Layout::new::<InlineBackingArray<T>>();
        let array = Layout::array::<T>(length as usize).expect("capacity overflow");
        let (layout, offset) = header.extend(array).expect("capacity overflow");
        debug_assert_eq!(offset, Self::array_offset());
        (layout.pad_to_align(), offset)
    }

    /// # Safety
    /// `T` must be valid when zero-initialized and must not require drop.
    pub(crate) unsafe fn new(metadata: HistogramStorageMetadata) -> *mut InlineBackingArray<T> {
        let (layout, _) = InlineBackingArray::<T>::layout(metadata.counts_array_length);
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        let res_ptr = ptr as *mut InlineBackingArray<T>;
        (*res_ptr).length = metadata.counts_array_length;
        (*res_ptr).bucket_count = metadata.bucket_count;
        (*res_ptr).highest_trackable_value = metadata.highest_trackable_value;
        (*res_ptr).normalizing_index_offset = AtomicI32::new(0);
        (*res_ptr).double_to_integer_value_conversion_ratio = AtomicU64::new(1.0_f64.to_bits());
        res_ptr
    }

    #[inline(always)]
    pub(crate) unsafe fn get_array_ptr(&self) -> *mut T {
        (self as *const InlineBackingArray<T> as *const u8).add(Self::array_offset()) as *mut T
    }

    #[inline(always)]
    pub(crate) fn length(&self) -> u32 {
        self.length
    }

    #[inline(always)]
    pub(crate) fn metadata(&self) -> HistogramStorageMetadata {
        HistogramStorageMetadata {
            bucket_count: self.bucket_count,
            counts_array_length: self.length,
            highest_trackable_value: self.highest_trackable_value,
        }
    }

    #[inline(always)]
    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        self.normalizing_index_offset.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        self.normalizing_index_offset
            .store(util::normalize_index_offset(offset as i64, self.length), Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        1.0 / self.double_to_integer_value_conversion_ratio()
    }

    #[inline(always)]
    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        f64::from_bits(self.double_to_integer_value_conversion_ratio.load(Ordering::Relaxed))
    }

    #[inline(always)]
    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        self.double_to_integer_value_conversion_ratio
            .store((1.0 / ratio).to_bits(), Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn get(&self, index: u32) -> Option<&T> {
        if index < self.length {
            return unsafe { Some(self.get_unchecked(index)) };
        }
        None
    }

    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, index: u32) -> &T {
        &*self.get_array_ptr().add(index as usize)
    }

    pub(crate) fn dealloc(&mut self) {
        let (layout, _) = InlineBackingArray::<T>::layout(self.length);
        unsafe { dealloc(self as *mut InlineBackingArray<T> as *mut u8, layout) };
    }
}
