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
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("beckon.rc", embed_resource::NONE);
    }
}
