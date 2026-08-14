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
        embed_resource::compile("examples.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}

#[cfg(not(windows))]
fn embed_win32_resources() {}
