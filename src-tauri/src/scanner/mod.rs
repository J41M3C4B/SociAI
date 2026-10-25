//! The local detector lives in the `privacy` crate (ADR-004), which the platform uses too. The app names it here as it
//! always did, and keeps the guard of its forms, which writes in the app's audit log.

pub use privacy::*;

pub mod guard;
