#![allow(unused_assignments)] // thiserror/miette proc macros trigger false positives

pub mod cloudinit;
pub mod config;
pub mod guest;
pub mod error;
pub mod image;
pub mod instance;
pub mod iso9660;
pub mod layout;
pub mod paths;
pub mod driver;
pub mod qcow2;
pub mod util;
