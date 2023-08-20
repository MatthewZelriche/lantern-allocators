#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]
#![feature(allocator_api)]

pub mod memory_segmenter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
