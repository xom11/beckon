fn main() {
    println!("cargo:rerun-if-changed=examples.rc");
    println!("cargo:rerun-if-changed=examples.exe.manifest");
    embed_win32_resources();
}

/// Give this package's **examples** the same activation context the shipped
/// binaries get.
///
/// `beckon-cli/build.rs` embeds a manifest and says it "applies to every
/// binary in the package" -- which is true, and the package is `beckon-cli`.
/// The probes live in `beckon-windows`, a different package, so until now
/// every one of them ran with **no manifest at all**: comctl32 **v5**,
/// DPI-unaware.
///
/// **That is not a cosmetic difference, it is a measurement error.** Under v5
/// a BUTTON sends no `NM_CUSTOMDRAW` whatsoever, so `examples/pill_probe.rs`
/// -- gate G2, which exists to decide whether `CDIS_HOT` reaches a
/// `BS_PUSHLIKE` auto-radio -- came back with `notifications: radio=0 push=0`
/// on a14 2026-08-14. Both zero, including the plain `BS_PUSHBUTTON` that is
/// the probe's own control. The control is what caught it: without one the
/// run reads as "a pushlike radio never goes hot", which would have sent the
/// tab strip back to a fallback design it does not need.
///
/// A probe that measures a different comctl32 from the product answers a
/// question nobody asked. Note `examples/combo_probe.rs` already treats
/// "comctl32 v5 vs v6" as a variable it controls for; this makes the default
/// match the product rather than leaving each probe to remember.
///
/// MSVC only, for the same reason `beckon-cli`'s is: `embed-resource` shells
/// out to `rc.exe`, the dev host has neither that nor `windres`, and the
/// local Windows gate cross-checks `x86_64-pc-windows-gnu`. A -gnu build
/// keeps the old behaviour, which is acceptable because nothing is released
/// from one.
#[cfg(windows)]
fn embed_win32_resources() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // `compile` is the WRONG function here and the difference is invisible
        // until you ask the running process what it loaded. It emits
        // `cargo:rustc-link-arg-bins`, and **an example is not a bin** -- so
        // the resource is built, the build succeeds, and the example links
        // without it. Measured on a14 2026-08-14: with `compile`, the probe's
        // own `DllGetVersion` reported comctl32 **5.82.10586**, which is why
        // every NM_CUSTOMDRAW count was zero.
        //
        // `compile_for` names the targets explicitly. Every example that opens
        // a window belongs in this list; one that does not will silently
        // measure v5 again.
        //
        // (`embed-resource` 2.x returns `()`; the `.manifest_optional()`
        // builder is 3.x. This file shipped with the 3.x form once and the
        // macOS cross-check said nothing, because a build script is compiled
        // for the HOST -- `#[cfg(windows)]` is dead code on the gate machine,
        // so anything inside it is only ever type-checked by a real Windows
        // build.)
        embed_resource::compile_for(
            "examples.rc",
            [
                "pill_probe",
                "settings_probe",
                "combo_probe",
                "caps_probe",
                "caps_live",
                "customdraw_probe",
                "showhide_probe",
            ],
            embed_resource::NONE,
        );
    }
}

#[cfg(not(windows))]
fn embed_win32_resources() {}
