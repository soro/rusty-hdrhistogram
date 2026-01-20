use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicI32, Ordering};

#[repr(C)]
pub(crate) struct InlineBackingArray<T> {
    length: u32,
    normalizing_index_offset: AtomicI32,
    _marker: PhantomData<T>,
}

impl<T> InlineBackingArray<T> {
    fn layout(length: u32) -> (Layout, usize) {
        let header = Layout::new::<InlineBackingArray<T>>();
        let array = Layout::array::<T>(length as usize).expect("capacity overflow");
        let (layout, offset) = header.extend(array).expect("capacity overflow");
        (layout.pad_to_align(), offset)
    }

    /// # Safety
    /// `T` must be valid when zero-initialized and must not require drop.
    pub(crate) unsafe fn new(length: u32) -> *mut InlineBackingArray<T> {
        let (layout, _) = InlineBackingArray::<T>::layout(length);
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        let res_ptr = ptr as *mut InlineBackingArray<T>;
        (*res_ptr).length = length;
        (*res_ptr).normalizing_index_offset = AtomicI32::new(0);
        res_ptr
    }

    pub(crate) unsafe fn get_array_ptr(&self) -> *mut T {
        let (_, offset) = InlineBackingArray::<T>::layout(self.length);
        (self as *const InlineBackingArray<T> as *const u8)
            .add(offset) as *mut T
    }

    pub(crate) fn length(&self) -> u32 {
        self.length
    }

    #[inline(always)]
    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        self.normalizing_index_offset.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        self.normalizing_index_offset.store(offset, Ordering::Relaxed);
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
