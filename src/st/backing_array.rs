use crate::core::HistogramStorageMetadata;

/// Owned counts with no spare capacity or separately cached length. The header
/// lives in the histogram; growing replaces only the counts allocation.
pub struct BackingArray<T> {
    data: Box<[T]>,
    bucket_count: u32,
    highest_trackable_value: u64,
}

impl<T: Default + Copy> BackingArray<T> {
    #[inline]
    pub fn new(metadata: HistogramStorageMetadata) -> BackingArray<T> {
        BackingArray {
            data: vec![T::default(); metadata.counts_array_length as usize].into_boxed_slice(),
            bucket_count: metadata.bucket_count,
            highest_trackable_value: metadata.highest_trackable_value,
        }
    }

    pub fn empty() -> BackingArray<T> {
        BackingArray {
            data: Box::default(),
            bucket_count: 0,
            highest_trackable_value: 0,
        }
    }

    #[inline]
    pub fn grow(&mut self, metadata: HistogramStorageMetadata) {
        let new_length = metadata.counts_array_length as usize;
        assert!(new_length >= self.data.len(), "backing array cannot shrink");
        if new_length > self.data.len() {
            let mut data = vec![T::default(); new_length].into_boxed_slice();
            data[..self.data.len()].copy_from_slice(&self.data);
            self.data = data;
        }
        self.bucket_count = metadata.bucket_count;
        self.highest_trackable_value = metadata.highest_trackable_value;
    }

    #[inline(always)]
    pub fn get(&self, index: u32) -> Option<&T> {
        self.data.get(index as usize)
    }

    #[inline(always)]
    pub fn get_unchecked(&self, index: u32) -> &T {
        unsafe { self.data.get_unchecked(index as usize) }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        self.data.get_mut(index as usize)
    }

    #[inline(always)]
    pub fn get_unchecked_mut(&mut self, index: u32) -> &mut T {
        unsafe { self.data.get_unchecked_mut(index as usize) }
    }

    #[inline(always)]
    pub fn length(&self) -> u32 {
        self.data.len() as u32
    }

    #[inline(always)]
    pub fn metadata(&self) -> HistogramStorageMetadata {
        HistogramStorageMetadata {
            bucket_count: self.bucket_count,
            counts_array_length: self.length(),
            highest_trackable_value: self.highest_trackable_value,
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        for value in &mut self.data {
            *value = T::default();
        }
    }

    pub fn get_slice(&self, length: u32) -> Option<&[T]> {
        let length = length as usize;
        if length <= self.data.len() {
            return Some(&self.data[..length]);
        }
        None
    }

    pub fn get_slice_mut(&mut self, length: u32) -> Option<&mut [T]> {
        let length = length as usize;
        if length <= self.data.len() {
            return Some(&mut self.data[..length]);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(length: u32) -> HistogramStorageMetadata {
        HistogramStorageMetadata {
            counts_array_length: length,
            bucket_count: length,
            highest_trackable_value: u64::from(length) * 1_024,
        }
    }

    #[test]
    fn owned_slice_grows_preserving_counts_and_deriving_metadata_length() {
        let mut counts = BackingArray::<u64>::empty();
        assert_eq!(counts.length(), 0);
        assert_eq!(counts.metadata(), metadata(0));
        counts.grow(metadata(3));
        *counts.get_unchecked_mut(0) = 4;
        *counts.get_unchecked_mut(2) = 9;
        counts.grow(metadata(7));
        assert_eq!(counts.length(), 7);
        assert_eq!(counts.metadata(), metadata(7));
        assert_eq!(counts.get_slice(7).unwrap(), &[4, 0, 9, 0, 0, 0, 0]);
        assert!(counts.get_slice(8).is_none());
        assert!(counts.get(7).is_none());
        counts.clear();
        assert_eq!(counts.get_slice(7).unwrap(), &[0; 7]);
        assert_eq!(counts.metadata(), metadata(7));
    }

    #[test]
    fn unchanged_slice_length_updates_metadata_without_reallocation() {
        let mut counts = BackingArray::<u16>::new(metadata(3));
        *counts.get_unchecked_mut(1) = 12;
        let original_ptr = counts.data.as_ptr();
        let updated = HistogramStorageMetadata {
            highest_trackable_value: 4_000,
            ..metadata(3)
        };
        counts.grow(updated);
        assert_eq!(counts.data.as_ptr(), original_ptr);
        assert_eq!(counts.metadata(), updated);
        assert_eq!(counts.get_slice(3).unwrap(), &[0, 12, 0]);
    }
}
