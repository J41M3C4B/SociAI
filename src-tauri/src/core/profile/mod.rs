//! The profile of the institution: who it is, its capacity and quick figures, and the anonymous lines the modules
//! add up to (`sync`). Its data are the module Mi institución (ERP-1); this is how the rest of the app reads them, and
//! how a database of before the modules hands them over (`adopt`).

pub mod adopt;
pub mod domain;
pub mod forms;
pub mod service;
pub mod storage;
pub mod sync;
