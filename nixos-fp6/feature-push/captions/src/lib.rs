#![forbid(unsafe_code)]
pub mod engine;
pub mod models;
pub mod service;
pub mod storage;
pub mod streaming;
pub const BUS: &str = "com.fairphone6.Captions1";
pub const PATH: &str = "/com/fairphone6/Captions1";
