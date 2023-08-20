use core::{
    alloc::{AllocError, Allocator, Layout},
    cell::UnsafeCell,
    ptr::NonNull,
    slice::from_raw_parts_mut,
};

use crate::memory_segmenter::{MemorySegmenter, SegmentMetadata};

struct LinkedListAllocImpl {
    segmenter_list: MemorySegmenter,
}

pub struct LinkedListAlloc(UnsafeCell<LinkedListAllocImpl>);

impl LinkedListAlloc {
    pub unsafe fn new(start: *mut u8, end: *mut u8) -> Self {
        let internal = LinkedListAllocImpl {
            segmenter_list: unsafe { MemorySegmenter::new(start, end) },
        };

        LinkedListAlloc(UnsafeCell::new(internal))
    }

    fn get_mut_internal(&self) -> &mut MemorySegmenter {
        &mut unsafe { self.0.get().as_mut() }.unwrap().segmenter_list
    }

    fn get_internal(&self) -> &MemorySegmenter {
        unsafe { &self.0.get().as_ref().unwrap().segmenter_list }
    }
}

unsafe impl Allocator for LinkedListAlloc {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        for entry in self.get_internal().iter() {
            if entry.size_allocable() < layout.size() {
                continue;
            }

            let candidate = unsafe {
                self.get_mut_internal().create_used_segment(
                    entry.addr().cast_mut(),
                    layout.size() + SegmentMetadata::SIZE,
                    layout.align(),
                )
            };

            if let Ok(new_segment) = candidate {
                let user_ptr = unsafe { new_segment.add(1) as *mut u8 };
                let user_slice =
                    unsafe { from_raw_parts_mut(user_ptr, layout.size()) } as *mut [u8];

                return Ok(NonNull::new(user_slice).unwrap());
            }
        }

        Err(AllocError)
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _: Layout) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    #[test]
    fn ll_allocator_tests() {
        const MIB: usize = 1048576;
        const SIZE: usize = 2 * MIB;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(SIZE, 16).unwrap()) };

        {
            let allocator = unsafe { LinkedListAlloc::new(mem, mem.add(SIZE)) };
            // Attempt to allocate larger than we can hold
            let res = allocator.allocate(Layout::from_size_align(SIZE, 16).unwrap());
            assert_eq!(res.is_err(), true);

            // Attempt to allocate exactly as much as we can hold
            let res = unsafe {
                allocator
                    .allocate(Layout::from_size_align(SIZE - SegmentMetadata::SIZE, 16).unwrap())
                    .unwrap()
                    .as_mut()
            };
            assert_eq!(res.as_ptr().align_offset(16), 0);
            assert_eq!(res.len(), SIZE - SegmentMetadata::SIZE);
            res.fill(0);
        }
    }
}
