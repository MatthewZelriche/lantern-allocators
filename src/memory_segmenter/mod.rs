use bit_field::BitField;
use core::{fmt::Debug, marker::PhantomData, mem::size_of, ptr::null_mut};

pub struct MemorySegmenter {
    head: *mut SegmentMetadata,
    start: *mut u8,
    end_exclusive: *mut u8,
}

pub struct MemorySegmenterIter<'a> {
    curr_segment: *mut SegmentMetadata,
    phantom: PhantomData<&'a SegmentMetadata>,
}

pub struct SegmentMetadata {
    prev: *mut SegmentMetadata,
    size: usize,
}

impl MemorySegmenter {
    pub unsafe fn new(start: *mut u8, end_exclusive: *mut u8) -> Self {
        let head = start as *mut SegmentMetadata;

        let this = MemorySegmenter {
            head,
            start,
            end_exclusive,
        };

        MemorySegmenter::write_metadata(
            head,
            SegmentMetadata::new(null_mut(), this.size(), false, false),
        );

        this
    }

    pub unsafe fn create_used_segment(
        &mut self,
        segment: *mut SegmentMetadata,
        subsegment_size: usize,
        required_align: usize,
    ) -> Result<*mut SegmentMetadata, ()> {
        let segment_bytes = segment as *mut u8;
        let segment_mut = segment.as_mut().unwrap();

        if subsegment_size > segment_mut.size() {
            return Err(());
        }

        // Can we utilize this segment as is, without having to create a new segment
        // to represent the used subsegment?
        if segment_bytes.align_offset(required_align) == 0 {
            segment_mut.set_in_use(true);

            // Did we use up the entire space of this segment?
            if segment_mut.size() == subsegment_size {
                // The easiest possible case - we are already done!
                return Ok(segment);
            }

            // We are truncating this segment, and building a new free segment immediately after...
            let old_size = segment_mut.size();
            let old_next_exists = segment_mut.next_exists();
            segment_mut.set_size(subsegment_size);
            let next_free_ptr = segment_bytes.add(segment_mut.size()) as *mut SegmentMetadata;
            let next_free_size = old_size - subsegment_size;
            MemorySegmenter::write_metadata(
                next_free_ptr,
                SegmentMetadata::new(segment, next_free_size, false, old_next_exists),
            );
            segment_mut.set_next_exists(true);

            // Fixup prevs
            let next_free_mut = MemorySegmenter::read_metadata(next_free_ptr);
            next_free_mut.set_prev(segment);
            next_free_mut
                .next()
                .and_then(|x| x.as_mut())
                .and_then(|x| Some(x.set_prev(next_free_ptr)));

            Ok(segment)
        } else {
            todo!()
        }
    }

    pub fn size(&self) -> usize {
        self.end_exclusive as usize - self.start as usize
    }

    pub fn iter(&self) -> MemorySegmenterIter {
        MemorySegmenterIter {
            curr_segment: self.head,
            phantom: PhantomData,
        }
    }

    unsafe fn write_metadata(dest: *mut SegmentMetadata, src: SegmentMetadata) {
        core::ptr::write(dest, src);
    }

    unsafe fn read_metadata(src: *mut SegmentMetadata) -> &'static mut SegmentMetadata {
        src.as_mut().unwrap()
    }
}

impl Debug for MemorySegmenter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for segment in self.iter() {
            write!(f, "{:?}", segment)?;
        }

        Ok(())
    }
}

impl<'a> Iterator for MemorySegmenterIter<'a> {
    type Item = &'a SegmentMetadata;

    fn next(&mut self) -> Option<Self::Item> {
        let item = unsafe { self.curr_segment.as_ref() }?;

        self.curr_segment = item.next().unwrap_or(null_mut());
        Some(item)
    }
}

impl Debug for SegmentMetadata {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "[prev: {:?}, size: {}, used: {}]",
            self.prev(),
            self.size(),
            self.in_use()
        )?;

        if self.next_exists() {
            write!(f, " -> ")
        } else {
            Ok(())
        }
    }
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

    pub fn set_prev(&mut self, prev: *mut SegmentMetadata) {
        self.prev = prev;
    }

    pub fn next(&self) -> Option<*mut SegmentMetadata> {
        self.next_exists()
            .then(|| unsafe { (self.addr() as *mut u8).add(self.size()) } as *mut SegmentMetadata)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use core::{alloc::Layout, ptr::null_mut};

    use super::*;

    #[test]
    fn segmenter() {
        const MIB: usize = 1048576;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(1024, MIB).unwrap()) };

        let segmenter = unsafe { MemorySegmenter::new(mem, mem.add(1024)) };

        println!("{:?}", segmenter);
    }

    #[test]
    fn segment_metadata() {
        const MIB: usize = 1048576;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(1024, MIB).unwrap()) };

        let segment1_ptr = mem as *mut SegmentMetadata;
        unsafe {
            core::ptr::write(
                segment1_ptr,
                SegmentMetadata::new(null_mut(), 64, true, false),
            )
        };
        let segment1_ref = unsafe { segment1_ptr.as_mut().unwrap() };
        assert_eq!(segment1_ref.addr(), segment1_ptr);
        assert_eq!(segment1_ref.alloc_start_ptr(), unsafe {
            (segment1_ptr as *mut u8).add(size_of::<SegmentMetadata>())
        });
        assert_eq!(segment1_ref.in_use(), true);
        assert_eq!(segment1_ref.next(), None);
        assert_eq!(segment1_ref.prev(), null_mut());
        assert_eq!(segment1_ref.size(), 64);
        assert_eq!(
            segment1_ref.size_allocable(),
            64 - size_of::<SegmentMetadata>()
        );
        let segment2_ptr = (unsafe { mem.add(64) } as *mut SegmentMetadata);
        unsafe {
            core::ptr::write(
                segment2_ptr,
                SegmentMetadata::new(segment1_ptr, 512, false, false),
            )
        };
        segment1_ref.set_next_exists(true);
        let segment2_ref = unsafe { segment1_ref.next().unwrap().as_mut() }.unwrap();
        assert_eq!(segment2_ref.addr(), segment2_ptr);
        assert_eq!(segment2_ref.alloc_start_ptr(), unsafe {
            (segment2_ptr as *mut u8).add(size_of::<SegmentMetadata>())
        });
        assert_eq!(segment2_ref.in_use(), false);
        assert_eq!(segment2_ref.next(), None);
        assert_eq!(segment2_ref.prev(), segment1_ptr);
        assert_eq!(segment2_ref.size(), 512);
        assert_eq!(
            segment2_ref.size_allocable(),
            512 - size_of::<SegmentMetadata>()
        );

        let segment3_ptr = (unsafe { mem.add(512 + 64) } as *mut SegmentMetadata);
        unsafe {
            core::ptr::write(
                segment3_ptr,
                SegmentMetadata::new(segment2_ptr, 32, false, false),
            )
        };
        segment2_ref.set_next_exists(true);
        let segment3_ref = unsafe { segment2_ref.next().unwrap().as_mut() }.unwrap();
        assert_eq!(segment3_ref.addr(), segment3_ptr);
        assert_eq!(segment3_ref.alloc_start_ptr(), unsafe {
            (segment3_ptr as *mut u8).add(size_of::<SegmentMetadata>())
        });
        assert_eq!(segment3_ref.in_use(), false);
        assert_eq!(segment3_ref.next(), None);
        assert_eq!(segment3_ref.prev(), segment2_ptr);
        assert_eq!(segment3_ref.size(), 32);
        assert_eq!(
            segment3_ref.size_allocable(),
            32 - size_of::<SegmentMetadata>()
        );
    }
}
