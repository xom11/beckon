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

    emit_version();
    embed_win32_resources();
}

/// Build `BECKON_VERSION` -- what `beckon --version` prints -- as
/// `<cargo version>` plus, when we can find one, ` (<short sha>)`.
///
/// The Cargo version alone cannot answer "which beckon is this?" for the way
/// this project is actually installed. A nix flake pins beckon to a *rev*,
/// and every rev between two releases reports the identical `0.9.4`: after
/// `nix flake update beckon` there is no way to tell whether the machine has
/// the fix that landed this morning. Measured this session -- a laptop on a
/// pinned input could not be distinguished from one three commits behind, and
/// the only way to answer it was to read `flake.lock` on the machine that
/// built it.
///
/// This closes half of a wider problem and it is worth saying which half.
/// The other half is that the RUNNING PROCESS may not be the image on disk:
/// on a14 a watchdog-started beckon ran the 0.8.0 image for three hours while
/// `--version` -- a *fresh* process, started from whatever is on disk today
/// -- printed 0.9.0. No version string can fix that, which is why the
/// settings window's About page compares `current_exe()`'s mtime against this
/// process's own start time instead. A sha makes `--version` specific; it
/// does not make it a report about a process you did not just start.
///
/// Two sources, in order, and **the environment wins**:
///
///  1. `BECKON_GIT_REV`. Required, not a convenience: `nix/package.nix`
///     filters `.git` out of `src`, so a nix build has no repository to ask
///     and `git` may not be on `$PATH` in the sandbox either. `flake.nix`
///     passes `self.shortRev` through instead.
///  2. `git rev-parse`, for a working checkout and for
///     `cargo install --git`, which does leave a repository behind.
///
/// Neither available -- a source tarball, a vendored tree -- and the version
/// is the bare Cargo one. That is a silent degrade on purpose: a build must
/// not fail for want of a cosmetic suffix.
///
/// **This function adds no `-dirty` marker of its own, deliberately.** The
/// suffix is baked when the build script runs, and the `rerun-if-changed`
/// lines below can name HEAD but cannot name "any file in the tree". A dirty
/// flag computed here would therefore go stale in the one direction that
/// matters -- claiming clean while it is not -- so source 2 claims only which
/// commit it was built from, which is a fact those paths can keep current.
///
/// Source 1 is a different matter and is passed through verbatim: nix
/// evaluates the whole tree at one instant, so it *can* tell, and it does.
/// Measured -- `nix eval .#beckon.BECKON_GIT_REV` on this worktree with four
/// modified files answered `400b452-dirty`, from `self.dirtyShortRev`. That
/// is a true statement nix is entitled to make and this one is not, which is
/// why the env var is taken as given rather than parsed or trimmed.
fn emit_version() {
    // The env var is source 1, so a change to it must re-run this script.
    println!("cargo:rerun-if-env-changed=BECKON_GIT_REV");

    let rev = std::env::var("BECKON_GIT_REV")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(git_rev);

    // `CARGO_PKG_VERSION` is `[workspace.package] version` -- every crate
    // here takes `version = { workspace = true }` -- so this cannot drift
    // from what `nix eval .#beckon.version` reads out of the same table.
    let base = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    match rev {
        Some(r) => println!("cargo:rustc-env=BECKON_VERSION={base} ({r})"),
        None => println!("cargo:rustc-env=BECKON_VERSION={base}"),
    }
}

/// The short sha of HEAD, or `None` for anything that is not a live checkout.
///
/// Every failure is `None`: no `git` on `$PATH`, not a repository, a repo
/// with no commits. `output()` already turns a missing binary into an `Err`
/// rather than a panic, and a non-zero status is checked separately because
/// git prints its diagnosis on stderr and exits non-zero with empty stdout.
///
/// The `rerun-if-changed` pair is what keeps the answer from going stale, and
/// it is the whole reason this function asks git for the PATHS rather than
/// assuming `.git/HEAD`: in a worktree `.git` is a *file*, HEAD lives under
/// `.git/worktrees/<name>/`, and the branch ref lives in the common dir -- so
/// a hardcoded relative path would name nothing, and cargo would keep serving
/// the sha from whenever the tree was first built. `--git-path` resolves both
/// correctly from anywhere inside either kind of checkout.
///
/// It answers in two different SHAPES and both are right, so do not "fix" one
/// into the other. Measured from `crates/beckon-cli`, which is the cwd cargo
/// gives a build script: a plain clone answers the relative `../../.git/HEAD`
/// and a linked worktree answers an absolute path under
/// `.git/worktrees/<name>/`. Cargo resolves a relative `rerun-if-changed`
/// against the package root -- the same directory -- so the relative form
/// lands on the real file; canonicalising it here would buy nothing and would
/// have to get the worktree case right by hand.
///
/// `cargo install --git` is the third shape and needs no special case: it
/// leaves a DETACHED HEAD, where `symbolic-ref` exits 1 and contributes
/// nothing, while `packed-refs` exists and does. Verified against a
/// `--depth 1` clone checked out detached.
///
/// The ref file is emitted only when it exists, because a packed ref has no
/// file of its own; `packed-refs` is named for that case. A path that does
/// not exist is not an error to cargo, but it does make the script re-run on
/// every single build, which is a cost paid for nothing.
fn git_rev() -> Option<String> {
    use std::process::Command;

    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!s.is_empty()).then_some(s)
    };

    let rev = git(&["rev-parse", "--short=7", "HEAD"])?;

    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(name) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(p) = git(&["rev-parse", "--git-path", &name]) {
            if std::path::Path::new(&p).exists() {
                println!("cargo:rerun-if-changed={p}");
            }
        }
    }
    if let Some(p) = git(&["rev-parse", "--git-path", "packed-refs"]) {
        if std::path::Path::new(&p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }

    Some(rev)
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
