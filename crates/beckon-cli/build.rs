fn main() {
    // MSVC only, deliberately. embed-resource shells out to a resource
    // compiler: rc.exe for -msvc, windres for -gnu. The dev host has
    // neither, and WINCHECK cross-checks against x86_64-pc-windows-gnu --
    // so compiling the resource unconditionally would break the project's
    // own local Windows gate on a machine that never ships a binary anyway.
    // The icon only has to exist in what we release, and every released
    // Windows artifact is -msvc.
    //
    // Applies to every binary in the package, so `beckon.exe` gets the icon
    // in Explorer too.
    // Both directives are required, and the icon one is the load-bearing
    // half. `embed-resource` documents that it emits no rerun-if-changed
    // annotation of its own, so with none here Cargo falls back to
    // "rescan the package directory" -- and `assets/` is at the repo root,
    // OUTSIDE this package. Editing the icon alone would then not rebuild
    // the resource, and a stale icon would stay embedded until an
    // unrelated change in `crates/beckon-cli/` or a `cargo clean`.
    //
    // Naming beckon.rc as well is belt-and-braces: the default heuristic
    // already covers it, but stating it keeps the two inputs symmetrical
    // for the next reader.
    println!("cargo:rerun-if-changed=../../assets/beckon.ico");
    println!("cargo:rerun-if-changed=beckon.rc");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("beckon.rc", embed_resource::NONE);
    }
}
