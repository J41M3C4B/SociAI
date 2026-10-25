//! The capability contract and the registry (ADR-001). A module publishes what it offers as capabilities, each one
//! declared with `capability!` next to the function that does the work; the registry finds them all when the program
//! is linked. Nothing here knows the AI exists, and no module uses the AI: the platform uses the capabilities.
//!
//! A contract is whole or it does not compile. This one has every field:
//!
//! ```no_run
//! use capability::{Call, CapabilityError, Limits};
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct Ask {}
//!
//! #[derive(serde::Serialize, schemars::JsonSchema)]
//! struct Missing { fields: Vec<String> }
//!
//! fn missing(_: &Call, _: Ask) -> Result<Missing, CapabilityError> {
//!     Ok(Missing { fields: vec![] })
//! }
//!
//! capability::capability! {
//!     static MISSING = "institution.missing_fields" {
//!         description: "Qué datos le faltan a la institución, por ventana.",
//!         effect: Query,
//!         permission: "kernel.usar",
//!         data: [Internal],
//!         approval: Never,
//!         limits: Limits::READ,
//!         run: read(missing),
//!     }
//! }
//! # fn main() {}
//! ```
//!
//! The same without the permission does not:
//!
//! ```compile_fail
//! use capability::{Call, CapabilityError, Limits};
//! # #[derive(serde::Deserialize, schemars::JsonSchema)]
//! # struct Ask {}
//! # #[derive(serde::Serialize, schemars::JsonSchema)]
//! # struct Missing { fields: Vec<String> }
//! # fn missing(_: &Call, _: Ask) -> Result<Missing, CapabilityError> { Ok(Missing { fields: vec![] }) }
//!
//! capability::capability! {
//!     static MISSING = "institution.missing_fields" {
//!         description: "Qué datos le faltan a la institución, por ventana.",
//!         effect: Query,
//!         data: [Internal],
//!         approval: Never,
//!         limits: Limits::READ,
//!         run: read(missing),
//!     }
//! }
//! # fn main() {}
//! ```
//!
//! Nor without the effect:
//!
//! ```compile_fail
//! use capability::{Call, CapabilityError, Limits};
//! # #[derive(serde::Deserialize, schemars::JsonSchema)]
//! # struct Ask {}
//! # #[derive(serde::Serialize, schemars::JsonSchema)]
//! # struct Missing { fields: Vec<String> }
//! # fn missing(_: &Call, _: Ask) -> Result<Missing, CapabilityError> { Ok(Missing { fields: vec![] }) }
//!
//! capability::capability! {
//!     static MISSING = "institution.missing_fields" {
//!         description: "Qué datos le faltan a la institución, por ventana.",
//!         permission: "kernel.usar",
//!         data: [Internal],
//!         approval: Never,
//!         limits: Limits::READ,
//!         run: read(missing),
//!     }
//! }
//! # fn main() {}
//! ```
//!
//! And a change whose `apply` takes another input than its `prepare` does not either:
//!
//! ```compile_fail
//! use capability::{Call, CapabilityError, Limits};
//! # #[derive(serde::Deserialize, schemars::JsonSchema)]
//! # struct Name { name: String }
//! # #[derive(serde::Deserialize, schemars::JsonSchema)]
//! # struct Other { other: String }
//! # #[derive(serde::Serialize, schemars::JsonSchema)]
//! # struct Done {}
//! # fn preview(_: &Call, _: Name) -> Result<Done, CapabilityError> { Ok(Done {}) }
//! # fn save(_: &mut Call, _: Other) -> Result<Done, CapabilityError> { Ok(Done {}) }
//!
//! capability::capability! {
//!     static RENAME = "institution.rename" {
//!         description: "Cambia el nombre de la institución.",
//!         effect: Modify,
//!         permission: "kernel.usar",
//!         data: [Public],
//!         approval: Required,
//!         limits: Limits::CHANGE,
//!         run: change(preview, save),
//!     }
//! }
//! # fn main() {}
//! ```

mod contract;
mod registry;
#[cfg(test)]
mod registry_tests;

pub use contract::*;
pub use registry::*;

// what `capability!` writes needs these from here, wherever it is used
#[doc(hidden)]
pub use linkme;
#[doc(hidden)]
pub use serde_json::Value;

/// Declares a capability of a module (ADR-001 §5): every field, in this order, and the functions that do the work.
/// A query or transformation runs with `read(f)`; a modification or sensitive action with `change(prepare, apply)`.
/// The schemas of the input and the output come from the types of those functions.
#[macro_export]
macro_rules! capability {
    (
        static $name:ident = $id:literal {
            description: $description:literal,
            effect: $effect:ident,
            permission: $permission:literal,
            data: [$($class:ident),* $(,)?],
            approval: $approval:ident,
            limits: $limits:expr,
            run: read($read:path) $(,)?
        }
    ) => {
        $crate::capability!(@declare $name, $id, $description, $effect, $permission, [$($class),*], $approval, $limits,
            $crate::Run::Read($crate::capability!(@step $read)), $read);
    };
    (
        static $name:ident = $id:literal {
            description: $description:literal,
            effect: $effect:ident,
            permission: $permission:literal,
            data: [$($class:ident),* $(,)?],
            approval: $approval:ident,
            limits: $limits:expr,
            run: change($prepare:path, $apply:path) $(,)?
        }
    ) => {
        const _: fn() = || $crate::same_input($prepare, $apply);
        $crate::capability!(@declare $name, $id, $description, $effect, $permission, [$($class),*], $approval, $limits,
            $crate::Run::Change {
                prepare: $crate::capability!(@step $prepare),
                apply: $crate::capability!(@apply $apply),
            },
            $prepare);
    };
    (@step $f:path) => {
        $crate::Step {
            call: {
                fn call(
                    call: &$crate::Call,
                    input: $crate::Value,
                ) -> ::std::result::Result<$crate::Value, $crate::CapabilityError> {
                    $crate::adapt(call, input, $f)
                }
                call
            },
            output: {
                fn output() -> $crate::Value {
                    $crate::output_schema($f)
                }
                output
            },
        }
    };
    (@apply $f:path) => {
        $crate::Apply {
            call: {
                fn call(
                    call: &mut $crate::Call,
                    input: $crate::Value,
                ) -> ::std::result::Result<$crate::Value, $crate::CapabilityError> {
                    $crate::adapt_apply(call, input, $f)
                }
                call
            },
            output: {
                fn output() -> $crate::Value {
                    $crate::apply_output_schema($f)
                }
                output
            },
        }
    };
    (@declare $name:ident, $id:literal, $description:literal, $effect:ident, $permission:literal, [$($class:ident),*],
        $approval:ident, $limits:expr, $run:expr, $first:path) => {
        #[$crate::linkme::distributed_slice($crate::CAPABILITIES)]
        #[linkme(crate = $crate::linkme)]
        static $name: $crate::Capability = $crate::Capability {
            id: $id,
            description: $description,
            effect: $crate::Effect::$effect,
            permission: $permission,
            data: &[$($crate::DataClass::$class),*],
            approval: $crate::Approval::$approval,
            limits: $limits,
            input: {
                fn input() -> $crate::Value {
                    $crate::input_schema($first)
                }
                input
            },
            run: $run,
        };
    };
}
