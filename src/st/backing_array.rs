use core::util::mem_util::{alloc_guard, alloc_zeroed_array_in, get_layout};
use std::{mem, slice};
use std::heap::{Alloc, Heap};
use std::ptr::{self, Unique};

pub struct BackingArray<T, A: Alloc = Heap> {
    pub(self) ptr: Unique<T>,
    length: u32,
    allocator: A,
}

impl<T, A: Alloc> BackingArray<T, A> {
    #[allow(unused_mut)]
    #[inline]
    pub fn new(length: u32, mut allocator: A) -> BackingArray<T, A> {
        BackingArray::with_length_in(length, allocator)
    }

    pub fn empty(allocator: A) -> BackingArray<T, A> {
        BackingArray {
            ptr: Unique::empty(),
            length: 0,
            allocator,
        }
    }

    // produce zeroed array of size length
    #[allow(unused_mut)]
    #[inline]
    fn with_length_in(length: u32, mut allocator: A) -> BackingArray<T, A> {
        unsafe {
            let ptr = alloc_zeroed_array_in::<T, A>(length, &mut allocator);

            BackingArray {
                ptr: Unique::new_unchecked(ptr as *mut _),
                length,
                allocator,
            }
        }
    }

    #[inline]
    pub unsafe fn grow(&mut self, new_length: u32) {
        // we just assume that new_length > self.length as this will always be the case for histograms
        alloc_guard(new_length as usize * mem::size_of::<T>());
        let old_layout = get_layout::<T>(self.length as usize);
        let new_layout = get_layout::<T>(new_length as usize);
        let new_ptr = self.allocator
            .realloc(self.ptr.as_ptr() as *mut u8, old_layout, new_layout);
        match new_ptr.map(|ptr| Unique::new_unchecked(ptr as *mut _)) {
            Ok(ptr) => {
                self.ptr = ptr;
                self.length = new_length;
            }
            Err(e) => self.allocator.oom(e),
        }
    }

    #[inline(always)]
    pub fn get(&self, index: u32) -> Option<&T> {
        if index < self.length {
            return Some(self.get_unchecked(index));
        }
        None
    }

    #[inline(always)]
    pub fn get_unchecked(&self, index: u32) -> &T {
        unsafe {
            let loc = self.ptr.as_ptr().offset(index as isize);
            &*loc
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        if index < self.length {
            return Some(self.get_unchecked_mut(index));
        }
        None
    }

    #[inline(always)]
    pub fn get_unchecked_mut(&mut self, index: u32) -> &mut T {
        unsafe {
            let loc = self.ptr.as_ptr().offset(index as isize);
            &mut *loc
        }
    }

    #[inline(always)]
    pub fn length(&self) -> u32 {
        self.length
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        unsafe {
            ptr::write_bytes(self.ptr.as_mut(), 0, self.length as usize);
        }
    }

    pub fn get_slice<'a>(&'a self, length: u32) -> Option<&'a [T]> {
        if length <= self.length {
            unsafe { return Some(slice::from_raw_parts(self.ptr.as_ptr(), length as usize)) };
        }
        None
    }

    pub fn get_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]> {
        if length <= self.length {
            unsafe {
                return Some(slice::from_raw_parts_mut(
                    self.ptr.as_ptr(),
                    length as usize,
                ));
            };
        }
        None
    }
}

impl<T, A: Alloc> Drop for BackingArray<T, A> {
    fn drop(&mut self) {
        unsafe {
            self.allocator.dealloc(
                self.ptr.as_ptr() as *mut u8,
                get_layout::<T>(self.length as usize),
            );
        }
    }
}
