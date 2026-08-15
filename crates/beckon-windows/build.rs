fn main() {
    println!("cargo:rerun-if-changed=examples.rc");
    println!("cargo:rerun-if-changed=examples.exe.manifest");
    stamp_target();
    embed_win32_resources();
}

/// Forward cargo's own `TARGET` into the crate as `BECKON_TARGET`, for the
/// About page's `Build` row.
///
/// **The exact triple, not a `cfg!`-derived one.** `std::env::consts::ARCH`
/// plus `cfg!(target_env)` gets `aarch64-pc-windows-msvc` right and cannot
/// see a vendor other than `pc` -- `aarch64-uwp-windows-msvc` and its
/// siblings would come back mislabelled. beckon does not build for those, so
/// this is a difference of one word in one row; it costs two lines here
/// because the build script already existed for the manifest above.
///
/// **It carries no build DATE, and that is a decision.** Design §3.4's
/// drawing shows one beside the triple. A date stamped here would really be
/// "when this build script last ran", which cargo caches -- so it can be
/// arbitrarily older than the binary beside it -- and it would make the
/// output non-reproducible for the Nix flake, which is a cost the row does
/// not repay: users install releases, so `beckon 0.9.3` on the row above
/// already answers "how old is this", and unlike a date it cannot disagree
/// with what the process is running.
///
/// Outside `embed_win32_resources`' `#[cfg(windows)]` deliberately: that
/// function is dead code on a macOS host, and the two Windows cross-check
/// legs of the gate compile this crate FROM one.
fn stamp_target() {
    println!(
        "cargo:rustc-env=BECKON_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".into())
    );
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
        // `compile_for` is also wrong: it emits `cargo:rustc-link-arg-bin=`
        // per name, so naming the examples there fails the build outright
        // ("does not have a bin target with the name `pill_probe`"). That one
        // at least announces itself.
        //
        // `compile_for_examples` emits `cargo:rustc-link-arg-examples`, which
        // is the directive that actually reaches an example target, and it
        // needs no list -- so an example added later cannot be forgotten and
        // silently measure v5.
        //
        // (`embed-resource` 2.x returns `()`; the `.manifest_optional()`
        // builder is 3.x. This file shipped with the 3.x form once and the
        // macOS cross-check said nothing, because a build script is compiled
        // for the HOST -- `#[cfg(windows)]` is dead code on the gate machine,
        // so anything inside it is only ever type-checked by a real Windows
        // build. All three mistakes here were found by running it.)
        embed_resource::compile_for_examples("examples.rc", embed_resource::NONE);
    }
}

#[cfg(not(windows))]
fn embed_win32_resources() {}
