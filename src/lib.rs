#![feature(test)]
#![feature(integer_atomics)]
#![feature(inclusive_range_syntax)]
#![feature(const_max_value)]
#![feature(allocator_api)]
#![feature(unique)]
#![feature(rustc_private)]
#![feature(const_atomic_usize_new)]
#![allow(dead_code)]
#![recursion_limit = "128"]

extern crate byteorder;
extern crate bytes;
extern crate miniz_oxide;
extern crate num_traits as num;
extern crate parking_lot as parking_lot;
extern crate base64;
extern crate lazycell;

#[macro_use]
mod core;
pub mod concurrent;
pub use core::errors::*;
pub mod st;
pub mod iteration;
pub mod serialization;
pub mod logging;

#[cfg(test)]
pub mod tests;
