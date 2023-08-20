use bit_field::BitField;
use core::mem::size_of;

pub struct MemorySegmenter {}

pub struct SegmentMetadata {
    prev: *mut SegmentMetadata,
    size: usize,
}

impl SegmentMetadata {
    const IN_USE_BIT: usize = 0;
    const NEXT_EXISTS_BIT: usize = 1;

    pub fn new(prev: *mut SegmentMetadata, size: usize, in_use: bool, next_exists: bool) -> Self {
        let mut this = SegmentMetadata { prev, size };
        this.set_in_use(in_use);
        this.set_next_exists(next_exists);

        this
    }

    pub fn addr(&self) -> *const SegmentMetadata {
        self as *const SegmentMetadata
    }

    pub fn set_size(&mut self, size: usize) {
        if size.get_bits(0..3) != 0 {
            panic!("Size must be a multiple of 8!");
        }
        self.size.set_bits(3.., size.get_bits(3..));
    }

    pub fn size(&self) -> usize {
        self.size.get_bits(3..) << 3
    }

    pub fn size_allocable(&self) -> usize {
        self.size() - size_of::<Self>()
    }

    pub fn alloc_start_ptr(&self) -> *mut u8 {
        (unsafe { self.addr().add(1) }) as *mut u8
    }

    pub fn set_in_use(&mut self, in_use: bool) {
        self.size.set_bit(Self::IN_USE_BIT, in_use);
    }

    pub fn in_use(&self) -> bool {
        self.size.get_bit(Self::IN_USE_BIT)
    }

    pub fn set_next_exists(&mut self, next_exists: bool) {
        self.size.set_bit(Self::NEXT_EXISTS_BIT, next_exists);
    }

    pub fn next_exists(&self) -> bool {
        self.size.get_bit(Self::NEXT_EXISTS_BIT)
    }

    pub fn prev(&self) -> *mut SegmentMetadata {
        self.prev
    }

    pub fn next(&self) -> Option<*mut SegmentMetadata> {
        self.next_exists()
            .then(|| unsafe { (self.addr() as *mut u8).add(self.size()) } as *mut SegmentMetadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
