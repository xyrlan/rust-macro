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

### Bundled Interception installer (binary redistribution, Plan 3d)

Built installers of this project (`.msi`, `.exe`) include the unmodified
binary `install-interception.exe` from oblitum's Interception project
(version `v1.0.1`, released 2017-05-12), redistributed under LGPL-3.0.

The bundled `crates/app/installers/interception/` directory contains:
- `install-interception.exe` — the installer/uninstaller (470 KB, unsigned).
  Invoke with `/install` to register the driver, `/uninstall` to remove.
- `LICENSE-LGPL.txt` — full LGPL-3.0 text (from upstream `licenses/non-commercial-usage/`).
- `SOURCE-INFO.txt` — pointer to upstream, version pin, SHA-256, usage notes.

**Note on signing:** the upstream `install-interception.exe` is **not**
digitally signed. Windows SmartScreen will display a warning on first
launch; the user must explicitly approve. This is a property of the
upstream binary, not a modification by rust-macro.

The LGPL "lesser license" obligations for binary redistribution are met
because:
1. The Interception driver loads as a Windows kernel filter; the rust-macro
   user-space process dynamically links to `interception.dll` only (loaded
   from `C:\Windows\System32` after install).
2. The user can replace the installed driver with a modified version
   without rebuilding rust-macro — uninstall the bundled version via the
   Settings page, reboot, install a custom Interception build.
3. Source/version info is included in the install directory.

For commercial usage of Interception (oblitum's dual-licensing model),
see `licenses/commercial-usage/` in the upstream release archive.

### Other deps

Workspace `Cargo.toml` lists all direct dependencies. Run `cargo license`
for a current full enumeration.
