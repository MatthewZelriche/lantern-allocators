use core::{
    alloc::{AllocError, Allocator, Layout},
    cell::UnsafeCell,
    ptr::NonNull,
};

use crate::memory_segmenter::{MemorySegmenter, SegmentMetadata};

struct LinkedListAllocImpl {
    segmenter_list: MemorySegmenter,
}

pub struct LinkedListAlloc(UnsafeCell<LinkedListAllocImpl>);

impl LinkedListAlloc {
    pub fn new(start: *mut u8, end: *mut u8) -> Self {
        let internal = LinkedListAllocImpl {
            segmenter_list: unsafe { MemorySegmenter::new(start, end) },
        };

        LinkedListAlloc(UnsafeCell::new(internal))
    }
}

unsafe impl Allocator for LinkedListAlloc {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        todo!()
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _: Layout) {
        todo!()
    }
}
