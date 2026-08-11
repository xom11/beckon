fn main() {
    // Both directives are required on every target, not just Windows ones,
    // and the icon one is the load-bearing half. `embed-resource` documents
    // that it emits no rerun-if-changed annotation of its own, so with none
    // here Cargo falls back to "rescan the package directory" -- and
    // `assets/` is at the repo root, OUTSIDE this package. Editing the icon
    // alone would then not rebuild the resource, and a stale icon would
    // stay embedded until an unrelated change in `crates/beckon-cli/` or a
    // `cargo clean`.
    //
    // Naming beckon.rc as well is belt-and-braces: the default heuristic
    // already covers it, but stating it keeps the two inputs symmetrical
    // for the next reader.
    println!("cargo:rerun-if-changed=../../assets/beckon.ico");
    println!("cargo:rerun-if-changed=beckon.rc");
    println!("cargo:rerun-if-changed=beckon.exe.manifest");

    embed_win32_resources();
}

/// MSVC only, deliberately. embed-resource shells out to a resource
/// compiler: rc.exe for -msvc, windres for -gnu. The dev host has neither,
/// and WINCHECK cross-checks against x86_64-pc-windows-gnu -- so compiling
/// the resource unconditionally would break the project's own local
/// Windows gate on a machine that never ships a binary anyway. The icon
/// only has to exist in what we release, and every released Windows
/// artifact is -msvc.
///
/// `beckon.rc` names two resources, both gated by the same MSVC-only check:
/// `1 ICON` (the icon) and `1 24` (`RT_MANIFEST`, `beckon.exe.manifest`).
/// The manifest is not cosmetic like the icon -- it is what puts the process
/// under per-monitor-v2 DPI awareness and a v6 comctl32 activation context,
/// and `ui_font`'s `SystemParametersInfoForDpi`-vs-`SystemParametersInfoW`
/// fallback in `settings_window.rs` exists specifically because a -gnu build
/// (or `cargo install --git` with no resource compiler) skips this function
/// and stays DPI-unaware. Skipping the manifest on those builds is accepted,
/// not fixed, for the same reason skipping the icon is: every released
/// Windows artifact is -msvc.
///
/// Applies to every binary in the package, so `beckon.exe` gets the icon in
/// Explorer, and comctl32 v6 / per-monitor-v2, too -- see Task 10 in
/// `docs/superpowers/plans/2026-08-11-settings-window-landing-1.md` for why
/// that also needs a hot-path re-measurement, not just a settings-window one.
///
/// `#[cfg(windows)]`-gated, matching the `embed-resource` build-dependency
/// now being scoped to `target.'cfg(windows)'.build-dependencies` in
/// Cargo.toml: on a Linux/macOS host the crate is not even a dependency of
/// this build, so a call into it has to be compiled out here, not just
/// skipped at runtime.
#[cfg(windows)]
fn embed_win32_resources() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("beckon.rc", embed_resource::NONE);
    }
}

#[cfg(not(windows))]
fn embed_win32_resources() {}
