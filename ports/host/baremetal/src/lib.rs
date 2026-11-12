//! Bare-metal host layer for gust (rework step 3, docs/architecture/three-modes-two-axes.md).
//!
//! What a gust binary needs from "no operating system underneath": today, the debug
//! console its demonstrators report through. Boot, exit and fault entry follow.
#![no_std]

pub mod console;
