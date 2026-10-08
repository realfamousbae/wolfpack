//! The declarations of the standard library (spec chapter 7).
//!
//! The std modules are ordinary `.wolf` files compiled into the compiler. Functions, methods and
//! types marked `intrinsic` have no body; the backends implement them.

/// One module of the standard library.
#[derive(Clone, Copy, Debug)]
pub struct StdModule {
    /// The module path, for example `std.io`.
    pub path: &'static str,
    /// The name shown in diagnostics, for example `<std>/io.wolf`.
    pub file_name: &'static str,
    /// The source text.
    pub source: &'static str,
}

/// The path of the prelude module. It cannot be imported; its public names (except `Never`)
/// are visible everywhere (spec chapter 6, section 3.3).
pub const PRELUDE: &str = "std.prelude";

/// Names of the prelude module that are not visible to users.
pub const HIDDEN_PRELUDE_NAMES: &[&str] = &["Never"];

macro_rules! module {
    ($path:literal, $file:literal) => {
        StdModule {
            path: $path,
            file_name: concat!("<std>/", $file),
            source: include_str!(concat!("../std/", $file)),
        }
    };
}

/// Every std module, in a fixed order.
pub const MODULES: &[StdModule] = &[
    module!("std.prelude", "prelude.wolf"),
    module!("std.io", "io.wolf"),
    module!("std.string", "string.wolf"),
    module!("std.list", "list.wolf"),
    module!("std.map", "map.wolf"),
    module!("std.math", "math.wolf"),
    module!("std.fs", "fs.wolf"),
    module!("std.process", "process.wolf"),
];
