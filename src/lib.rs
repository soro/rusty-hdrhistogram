#![feature(test)]
#![allow(dead_code)]
#![recursion_limit = "128"]


#[macro_use]
mod core;
pub mod concurrent;
pub use crate::core::errors::*;
pub mod st;
pub mod iteration;

#[cfg(test)]
pub mod tests;
