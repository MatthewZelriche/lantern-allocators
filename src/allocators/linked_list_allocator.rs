use core::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
    slice::from_raw_parts_mut,
};

use crate::memory_segmenter::{MemorySegmenter, SegmentMetadata};

#[derive(Debug)]
struct LinkedListAllocImpl {
    segmenter_list: MemorySegmenter,
}

#[derive(Debug)]
pub struct LinkedListAlloc<R: lock_api::RawMutex>(lock_api::Mutex<R, LinkedListAllocImpl>);

unsafe impl<R: lock_api::RawMutex> Send for LinkedListAlloc<R> {}

impl<R: lock_api::RawMutex> LinkedListAlloc<R> {
    pub unsafe fn new(start: *mut u8, end: *mut u8) -> Self {
        let internal = LinkedListAllocImpl {
            segmenter_list: unsafe { MemorySegmenter::new(start, end) },
        };

        LinkedListAlloc(lock_api::Mutex::new(internal))
    }
}

unsafe impl<R: lock_api::RawMutex> Allocator for LinkedListAlloc<R> {
    fn allocate(&self, layout: Layout) -> Result<core::ptr::NonNull<[u8]>, AllocError> {
        let mut internal = self.0.lock();

        let real_align = layout.align().max(SegmentMetadata::SIZE);
        // Round size request to nearest SIZE byte boundary
        let real_layout_size = layout.size().next_multiple_of(SegmentMetadata::SIZE);
        let subsegment_size = real_layout_size + SegmentMetadata::SIZE;
        let mut valid_segment_ptr = None;

        for entry in internal.segmenter_list.iter() {
            if entry.size_allocable() < real_layout_size {
                continue;
            }

            if internal
                .segmenter_list
                .calculate_alloc_ptr_with_required_align(entry, subsegment_size, real_align)
                .is_err()
            {
                continue;
            }

            // Found a valid segment to split
            valid_segment_ptr = Some(entry.addr());
        }

        if let Some(valid_segment_ptr) = valid_segment_ptr {
            let candidate = unsafe {
                internal.segmenter_list.create_used_segment(
                    valid_segment_ptr.cast_mut().as_mut().unwrap(),
                    subsegment_size,
                    real_align,
                )
            };

            if let Ok(new_segment) = candidate {
                let user_ptr = unsafe { new_segment.as_mut() }.unwrap().alloc_start_ptr();
                let user_slice =
                    unsafe { from_raw_parts_mut(user_ptr, real_layout_size) } as *mut [u8];

                Ok(NonNull::new(user_slice).unwrap())
            } else {
                Err(AllocError)
            }
        } else {
            Err(AllocError)
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _: Layout) {
        let mut internal = self.0.lock();

        // Get segment start
        let segment_start_ptr = (ptr.as_ptr() as *mut SegmentMetadata).sub(1);
        internal
            .segmenter_list
            .delete_used_segment(segment_start_ptr)
            .expect("Failed to free data!");
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use core::mem::size_of;

    use rand::{thread_rng, Rng};

    use super::*;

    #[test]
    fn ll_allocator_tests() {
        const MIB: usize = 1048576;
        const SIZE: usize = 2 * MIB;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(SIZE, 16).unwrap()) };

        {
            let allocator: LinkedListAlloc<parking_lot::RawMutex> =
                unsafe { LinkedListAlloc::new(mem, mem.add(SIZE)) };
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
            res.fill(0);
            assert_eq!(res.as_ptr().align_offset(16), 0);
            assert_eq!(res.len(), SIZE - SegmentMetadata::SIZE);
        }

        {
            let allocator: LinkedListAlloc<parking_lot::RawMutex> =
                unsafe { LinkedListAlloc::new(mem, mem.add(SIZE)) };

            let mut allocs = Vec::new();

            // Allocate randomly until we no longer can:
            let mut rng = thread_rng();
            loop {
                let mut random_size: usize = rng.gen_range(8..=1024);
                random_size = random_size.next_multiple_of(SegmentMetadata::SIZE);
                let random_alignment: usize = 2usize.pow(rng.gen_range(3..=10));

                let res = unsafe {
                    allocator
                        .allocate(Layout::from_size_align(random_size, random_alignment).unwrap())
                };

                if res.is_err() {
                    break;
                }

                let mem = unsafe { res.unwrap().as_mut() };
                mem.fill(0);
                assert_eq!(mem.as_ptr().align_offset(random_alignment), 0);
                assert_eq!(mem.len(), random_size);
                allocs.push(mem.as_ptr());

                // Metadata should be immediately before the ptr...
                let metadata = mem.as_mut_ptr() as *mut SegmentMetadata;
                let metadata = unsafe { metadata.sub(1) };
                let metadata_mut = unsafe { metadata.as_mut().unwrap() };
                assert_eq!(metadata_mut.size_allocable(), random_size);
            }

            // If we didnt allocate at least this many times, something very likely went wrong...
            assert!(allocs.len() > 1000);

            // Deallocate in a random order
            while allocs.len() > 0 {
                let idx = rng.gen_range(0..allocs.len());
                let ptr = allocs.swap_remove(idx);

                unsafe {
                    allocator.deallocate(
                        NonNull::new(ptr.cast_mut()).unwrap(),
                        Layout::from_size_align(8, 8).unwrap(),
                    );
                }
            }
            assert_eq!(
                allocator.0.lock().segmenter_list.overhead(),
                SegmentMetadata::SIZE
            );

            // Try to allocate entire memory to ensure we successfully deallocated everything
            let mem = unsafe {
                allocator
                    .allocate(Layout::from_size_align(SIZE - SegmentMetadata::SIZE, 16).unwrap())
                    .unwrap()
                    .as_ptr()
                    .as_mut()
                    .unwrap()
            };
            mem.fill(0);
            assert_eq!(mem.as_ptr().align_offset(16), 0);
            assert_eq!(mem.len(), SIZE - SegmentMetadata::SIZE);

            unsafe {
                allocator.deallocate(
                    NonNull::new(mem.as_ptr().cast_mut()).unwrap(),
                    Layout::from_size_align(8, 8).unwrap(),
                );
            }

            // Test a weird, nonstandard small size and alignment
            let mem = unsafe {
                {
                    allocator
                        .allocate(Layout::from_size_align(1, 2).unwrap())
                        .unwrap()
                        .as_ptr()
                        .as_mut()
                        .unwrap()
                }
            };
            assert_eq!(mem.len(), SegmentMetadata::SIZE);
            assert_eq!(mem.as_ptr().align_offset(SegmentMetadata::SIZE), 0);
        }
    }

    #[test]
    fn ll_allocator_vec() {
        const MIB: usize = 1048576;
        const SIZE: usize = 2 * MIB;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(SIZE, 16).unwrap()) };

        let allocator: LinkedListAlloc<parking_lot::RawMutex> =
            unsafe { LinkedListAlloc::new(mem, mem.add(SIZE)) };

        let mut vec = Vec::new_in(allocator);
        let mut rng = thread_rng();
        let mut total = 0;
        for _ in 0..1000 {
            let val: i32 = rng.gen_range(-100000..100000);
            total += val;
            vec.push(val);
        }
        assert_eq!(vec.len(), 1000);
        assert_eq!(vec.iter().sum::<i32>(), total);
    }

    #[test]
    fn ll_allocator_exceed_max() {
        const MIB: usize = 1048576;
        const SIZE: usize = MIB;
        let mem = unsafe { alloc::alloc::alloc(Layout::from_size_align(SIZE, 16).unwrap()) };

        let allocator: LinkedListAlloc<parking_lot::RawMutex> =
            unsafe { LinkedListAlloc::new(mem, mem.add(SIZE)) };

        let lower_bound = SIZE / size_of::<u64>();

        for i in 0..(lower_bound * 3) {
            let boxed_val = Box::new_in(i, &allocator);
            assert_eq!(*boxed_val, i);
        }
    }
}
