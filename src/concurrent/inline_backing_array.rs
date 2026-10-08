use crate::core::{util, HistogramStorageMetadata};
use std::alloc::{alloc_zeroed, handle_alloc_error, Layout};
use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering};

#[repr(C)]
struct Header {
    // Required to reconstruct the slice metadata after thin-pointer publication.
    // Immutable for the lifetime of this allocation; ordinary access uses the
    // slice's length instead.
    length: u32,
    bucket_count: u32,
    highest_trackable_value: u64,
    normalizing_index_offset: AtomicI32,
    double_to_integer_value_conversion_ratio: AtomicU64,
}

/// One allocation containing a header followed by the actual count elements.
/// References cover the whole allocation, including the trailing slice.
#[repr(C)]
pub(crate) struct InlineBackingArray<T> {
    header: Header,
    counts: [T],
}

impl<T> InlineBackingArray<T> {
    fn layout(length: u32) -> (Layout, usize) {
        let header = Layout::new::<Header>();
        let array = Layout::array::<T>(length as usize).expect("capacity overflow");
        let (layout, offset) = header.extend(array).expect("capacity overflow");
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
        ptr.cast::<Header>().write(Header {
            length: metadata.counts_array_length,
            bucket_count: metadata.bucket_count,
            highest_trackable_value: metadata.highest_trackable_value,
            normalizing_index_offset: AtomicI32::new(0),
            double_to_integer_value_conversion_ratio: AtomicU64::new(1.0_f64.to_bits()),
        });
        // Slice-tail pointer casts preserve length metadata. The data address
        // stays at the allocation base, not at the start of the counts.
        ptr::slice_from_raw_parts_mut(ptr.cast::<T>(), metadata.counts_array_length as usize) as *mut Self
    }

    #[inline(always)]
    pub(crate) unsafe fn get_array_ptr(&self) -> *mut T {
        self.counts.as_ptr() as *mut T
    }

    #[inline(always)]
    pub(crate) fn length(&self) -> u32 {
        self.counts.len() as u32
    }

    #[inline(always)]
    pub(crate) fn metadata(&self) -> HistogramStorageMetadata {
        HistogramStorageMetadata {
            bucket_count: self.header.bucket_count,
            counts_array_length: self.length(),
            highest_trackable_value: self.header.highest_trackable_value,
        }
    }

    #[inline(always)]
    pub(crate) fn normalizing_index_offset(&self) -> i32 {
        self.header.normalizing_index_offset.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub(crate) fn set_normalizing_index_offset(&self, offset: i32) {
        self.header
            .normalizing_index_offset
            .store(util::normalize_index_offset(offset as i64, self.length()), Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn integer_to_double_value_conversion_ratio(&self) -> f64 {
        1.0 / self.double_to_integer_value_conversion_ratio()
    }

    #[inline(always)]
    pub(crate) fn double_to_integer_value_conversion_ratio(&self) -> f64 {
        f64::from_bits(self.header.double_to_integer_value_conversion_ratio.load(Ordering::Relaxed))
    }

    #[inline(always)]
    pub(crate) fn set_integer_to_double_value_conversion_ratio(&self, ratio: f64) {
        self.header
            .double_to_integer_value_conversion_ratio
            .store((1.0 / ratio).to_bits(), Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn get(&self, index: u32) -> Option<&T> {
        self.counts.get(index as usize)
    }

    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, index: u32) -> &T {
        self.counts.get_unchecked(index as usize)
    }

    /// # Safety
    /// `ptr` must be an allocation returned by `new`, with its original slice
    /// metadata. All users and references must have finished, and the allocation
    /// must not have been freed already.
    pub(crate) unsafe fn dealloc(ptr: *mut Self) {
        // Box uses the full DST layout, including the counts, to free the single
        // allocation. Taking a raw pointer avoids freeing a live &mut receiver.
        drop(Box::from_raw(ptr));
    }
}

/// Thin atomic publication for an inline slice allocation. This does not own or
/// retire allocations: histogram owners retain their phaser/epoch obligations.
#[repr(transparent)]
pub(crate) struct AtomicInlineBackingArray<T> {
    ptr: AtomicPtr<Header>,
    _marker: PhantomData<T>,
}

impl<T> AtomicInlineBackingArray<T> {
    pub(crate) fn new(ptr: *mut InlineBackingArray<T>) -> Self {
        Self {
            ptr: AtomicPtr::new(ptr.cast::<Header>()),
            _marker: PhantomData,
        }
    }

    /// # Safety
    /// The loaded pointer must be a valid, initialized allocation of this type.
    /// The caller must prevent reclamation while the header is read, and for
    /// any subsequent use of the returned pointer (via the existing writer
    /// critical section, epoch guard, or exclusive access).
    /// Initialization must happen before this load through publication or other
    /// synchronization; the header's length cannot change after publication.
    #[inline(always)]
    pub(crate) unsafe fn load(&self, ordering: Ordering) -> *mut InlineBackingArray<T> {
        let ptr = self.ptr.load(ordering);
        let length = (*ptr).length as usize;
        // Retain allocation provenance: do not derive this pointer by casting
        // a reference to the sized header back to a raw pointer.
        ptr::slice_from_raw_parts_mut(ptr.cast::<T>(), length) as *mut InlineBackingArray<T>
    }

    #[inline(always)]
    pub(crate) fn store(&self, ptr: *mut InlineBackingArray<T>, ordering: Ordering) {
        self.ptr.store(ptr.cast::<Header>(), ordering);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, align_of_val, offset_of, size_of, size_of_val};

    fn metadata(length: u32) -> HistogramStorageMetadata {
        HistogramStorageMetadata {
            counts_array_length: length,
            bucket_count: 2,
            highest_trackable_value: 4_095,
        }
    }

    // T must satisfy InlineBackingArray::new's zero-initialization contract.
    unsafe fn check_layout<T>(length: u32) {
        let ptr = InlineBackingArray::<T>::new(metadata(length));
        {
            let array = &*ptr;
            let (layout, offset) = InlineBackingArray::<T>::layout(length);
            let counts = array.counts.as_ptr();
            assert_eq!(size_of_val(array), layout.size());
            assert_eq!(align_of_val(array), layout.align());
            assert_eq!(counts.cast::<u8>().offset_from(ptr.cast::<u8>()) as usize, offset);
            assert!(counts.is_aligned());
            assert_eq!(array.length(), length);
            assert_eq!(array.metadata(), metadata(length));
            assert!(array.get(length).is_none());
            for count in &array.counts {
                assert!(ptr::from_ref(count).is_aligned());
            }
        }
        InlineBackingArray::dealloc(ptr);
    }

    #[test]
    fn slice_layout_matches_allocation_for_empty_and_nonempty_tails() {
        #[repr(align(64))]
        struct Aligned(u8);

        for length in [0, 1, 3, 31] {
            unsafe {
                check_layout::<AtomicU64>(length);
                check_layout::<u8>(length);
                check_layout::<Aligned>(length);
                check_layout::<()>(length);
            }
        }
    }

    #[test]
    fn atomic_count_header_keeps_constant_aligned_offsets() {
        assert_eq!(size_of::<Header>(), 32);
        assert_eq!(align_of::<Header>(), 8);
        assert_eq!(offset_of!(Header, length), 0);
        assert_eq!(offset_of!(Header, normalizing_index_offset), 16);
        assert_eq!(offset_of!(Header, double_to_integer_value_conversion_ratio), 24);
        for length in [0, 1, 3, 2_048] {
            assert_eq!(InlineBackingArray::<AtomicU64>::layout(length).1, 32);
        }
        assert_eq!(size_of::<AtomicInlineBackingArray<AtomicU64>>(), size_of::<AtomicPtr<Header>>());
    }

    #[test]
    fn atomic_publication_recovers_each_allocations_slice_length() {
        unsafe {
            let first = InlineBackingArray::<AtomicU64>::new(metadata(3));
            let second = InlineBackingArray::<AtomicU64>::new(metadata(7));
            let published = AtomicInlineBackingArray::new(first);
            {
                let array = &*published.load(Ordering::Acquire);
                assert_eq!(array.length(), 3);
                assert_eq!(array.get_unchecked(2).fetch_add(5, Ordering::Relaxed), 0);
                array.set_normalizing_index_offset(1);
                array.set_integer_to_double_value_conversion_ratio(0.5);
            }
            published.store(second, Ordering::Release);
            {
                let array = &*published.load(Ordering::Acquire);
                assert_eq!(array.length(), 7);
                assert_eq!(array.get_unchecked(6).fetch_add(9, Ordering::Relaxed), 0);
                assert_eq!(array.normalizing_index_offset(), 0);
                assert_eq!(array.integer_to_double_value_conversion_ratio(), 1.0);
            }
            published.store(first, Ordering::Release);
            {
                let array = &*published.load(Ordering::Acquire);
                assert_eq!(array.get_unchecked(2).load(Ordering::Relaxed), 5);
                assert_eq!(array.normalizing_index_offset(), 1);
                assert_eq!(array.integer_to_double_value_conversion_ratio(), 0.5);
            }
            // No outstanding references or concurrent users remain.
            InlineBackingArray::dealloc(first);
            InlineBackingArray::dealloc(second);
        }
    }

    #[test]
    fn atomic_publication_keeps_address_and_length_together_under_readers() {
        unsafe {
            let first = InlineBackingArray::<AtomicU64>::new(metadata(3));
            let second = InlineBackingArray::<AtomicU64>::new(metadata(7));
            let published = AtomicInlineBackingArray::new(first);
            // Both allocations remain live until every reader has joined.
            // This isolates publication from the separately tested retirement
            // protocol and can run under both Miri borrow models.
            std::thread::scope(|scope| {
                for _ in 0..2 {
                    let published = &published;
                    scope.spawn(move || {
                        for _ in 0..16 {
                            let array = &*published.load(Ordering::Acquire);
                            let length = array.length();
                            assert!(length == 3 || length == 7);
                            array.get_unchecked(length - 1).fetch_add(1, Ordering::Relaxed);
                            std::thread::yield_now();
                        }
                    });
                }
                for _ in 0..16 {
                    published.store(second, Ordering::Release);
                    std::thread::yield_now();
                    published.store(first, Ordering::Release);
                }
            });
            assert_eq!(
                (*first).get_unchecked(2).load(Ordering::Relaxed) + (*second).get_unchecked(6).load(Ordering::Relaxed),
                32
            );
            InlineBackingArray::dealloc(first);
            InlineBackingArray::dealloc(second);
        }
    }
}
