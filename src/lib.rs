#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]
#![feature(allocator_api)]
#![feature(int_roundings)]

pub mod allocators;
pub mod memory_segmenter;
