# Licenses

This project is licensed under **MIT OR Apache-2.0** (see `Cargo.toml`).

## Third-party notices

### Interception (transitive, behind the `interception` Cargo feature)

When built with `--features interception`, this project depends on
`kanata-interception` (BSL-1.0), which in turn depends on `interception-sys`
(**LGPL-3.0**). `interception-sys` dynamically loads `interception.dll` from
the [Interception kernel driver project](https://github.com/oblitum/Interception)
at runtime.

**LGPL-3.0 implications.** Static linking from this project's binaries into
`interception-sys` (the Rust FFI wrapper) inherits LGPL obligations. If
distributing binaries built with `--features interception`, the LGPL requires
either:

1. Providing the source of `interception-sys` (it's freely available on
   [crates.io](https://crates.io/crates/interception-sys) and via
   `cargo vendor`), AND
2. Distributing in a form that allows the user to relink against a modified
   version of the LGPL component (for Rust, this typically means shipping
   the object files of the LGPL crate or making the build process reproducible
   from public sources).

For development and personal use, no distribution obligation applies.
For binary distribution, revisit the obligations or switch to a custom thin
FFI binding (option B in the design spec).

### Other deps

Workspace `Cargo.toml` lists all direct dependencies. Run `cargo license`
for a current full enumeration.
