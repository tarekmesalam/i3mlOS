//! A fixed-capacity inline vector. Used where the kernel must not allocate
//! (scheduler hot path, journal entries) — written here rather than pulled
//! from crates.io, per the purity charter.

#[derive(Clone, Copy)]
pub struct SmallVec<T, const N: usize> {
    items: [Option<T>; N],
    length: usize,
}

impl<T: Copy, const N: usize> SmallVec<T, N> {
    pub const fn new() -> Self {
        SmallVec { items: [None; N], length: 0 }
    }

    pub fn push(&mut self, item: T) -> bool {
        if self.length == N {
            return false;
        }
        self.items[self.length] = Some(item);
        self.length += 1;
        true
    }

    pub fn get(&self, index: usize) -> Option<T> {
        if index < self.length { self.items[index] } else { None }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn is_full(&self) -> bool {
        self.length == N
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.length).filter_map(move |index| self.items[index])
    }
}

impl<T: Copy, const N: usize> Default for SmallVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
