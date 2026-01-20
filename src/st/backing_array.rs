pub struct BackingArray<T> {
    data: Vec<T>,
}

impl<T: Default + Copy> BackingArray<T> {
    #[inline]
    pub fn new(length: u32) -> BackingArray<T> {
        BackingArray {
            data: vec![T::default(); length as usize],
        }
    }

    pub fn empty() -> BackingArray<T> {
        BackingArray { data: Vec::new() }
    }

    #[inline]
    pub fn grow(&mut self, new_length: u32) {
        let new_length = new_length as usize;
        if new_length > self.data.len() {
            self.data.resize(new_length, T::default());
        }
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
    pub fn clear(&mut self) {
        for value in &mut self.data {
            *value = T::default();
        }
    }

    pub fn get_slice<'a>(&'a self, length: u32) -> Option<&'a [T]> {
        let length = length as usize;
        if length <= self.data.len() {
            return Some(&self.data[..length]);
        }
        None
    }

    pub fn get_slice_mut<'a>(&'a mut self, length: u32) -> Option<&'a mut [T]> {
        let length = length as usize;
        if length <= self.data.len() {
            return Some(&mut self.data[..length]);
        }
        None
    }
}
