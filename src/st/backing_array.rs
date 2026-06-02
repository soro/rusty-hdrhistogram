use crate::core::HistogramStorageMetadata;

pub struct BackingArray<T> {
    data: Vec<T>,
    metadata: HistogramStorageMetadata,
}

impl<T: Default + Copy> BackingArray<T> {
    #[inline]
    pub fn new(metadata: HistogramStorageMetadata) -> BackingArray<T> {
        BackingArray {
            data: vec![T::default(); metadata.counts_array_length as usize],
            metadata,
        }
    }

    pub fn empty() -> BackingArray<T> {
        BackingArray {
            data: Vec::new(),
            metadata: HistogramStorageMetadata {
                bucket_count: 0,
                counts_array_length: 0,
                highest_trackable_value: 0,
            },
        }
    }

    #[inline]
    pub fn grow(&mut self, metadata: HistogramStorageMetadata) {
        let new_length = metadata.counts_array_length as usize;
        if new_length > self.data.len() {
            self.data.resize(new_length, T::default());
        }
        self.metadata = metadata;
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
        self.metadata.counts_array_length
    }

    #[inline(always)]
    pub fn metadata(&self) -> HistogramStorageMetadata {
        self.metadata
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
