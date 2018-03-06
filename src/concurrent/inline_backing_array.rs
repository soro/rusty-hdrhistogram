use core::util::mem_util::alloc_guard;
use std::fmt::Debug;
use std::heap::{self, Alloc, Layout};
use std::mem;

#[repr(C)]
pub struct InlineBackingArray<T, A: Alloc = heap::Heap> {
    length: u32,
    allocator: A,
    array: T,
}

impl<T: Debug, A: Alloc> InlineBackingArray<T, A> {
    fn get_size_and_layout(length: u32) -> (usize, Layout) {
        let t_size = mem::size_of::<T>();
        let array_size = if length == 0 {
            0
        } else {
            t_size * (length - 1).max(0) as usize
        };
        let size = mem::size_of::<InlineBackingArray<T, A>>() + array_size;

        unsafe {
            (
                size,
                Layout::from_size_align_unchecked(size, mem::align_of::<InlineBackingArray<T, A>>()),
            )
        }
    }

    pub unsafe fn new_in(length: u32, mut allocator: A) -> *mut InlineBackingArray<T, A> {
        let (size, layout) = InlineBackingArray::<T, A>::get_size_and_layout(length);

        alloc_guard(size);

        let res_ptr = match allocator.alloc_zeroed(layout) {
            Ok(ptr) => ptr as *mut InlineBackingArray<T, A>,
            Err(err) => allocator.oom(err),
        };

        (*res_ptr).length = length;

        res_ptr
    }

    pub unsafe fn get_array_ptr(&self) -> *mut T {
        &self.array as *const T as *mut T
    }

    pub fn length(&self) -> u32 {
        self.length
    }

    #[inline(always)]
    pub fn get(&self, index: u32) -> Option<&T> {
        if index < self.length {
            return unsafe { Some(self.get_unchecked(index)) };
        }
        None
    }

    #[inline(always)]
    pub unsafe fn get_unchecked(&self, index: u32) -> &T {
        let loc = (&self.array as *const T).offset(index as isize);
        &*loc
    }

    pub fn dealloc(&mut self) {
        let (_, layout) = InlineBackingArray::<T, A>::get_size_and_layout(self.length);
        let ptr: *mut u8 = unsafe { mem::transmute(&mut *self) };
        unsafe { self.allocator.dealloc(ptr, layout) };
    }
}
