# Distribution, packaging and the nix integration

Extracted from `CLAUDE.md` 2026-08-17. The install commands and the channel
list live there; this file is the detail behind each one.

## `beckon --version` carries the short sha, and the nix path is why

It prints `beckon <version> (<short sha>)` — `beckon 0.9.4 (400b452)` in the
measurements below, which is where the 0.9.4 in them comes from; the version
half tracks `[workspace.package]` as it always did. A flake input pins a *rev*,
so every rev between two releases reports the identical Cargo version: after
`nix flake update beckon` there was no way to ask a machine which commit it
had, and the only answer available was to read `flake.lock` on whatever built
it.

Three pieces, and each one exists because the obvious shorter version does not
work:

- `crates/beckon-cli/build.rs::emit_version` composes the string and
  `#[command(version = env!("BECKON_VERSION"))]` prints it. **Not the bare
  `version` attribute**, which is `CARGO_PKG_VERSION` alone.
- It reads `BECKON_GIT_REV` FIRST and falls back to `git rev-parse`. The env
  var is not a convenience: `nix/package.nix` filters `.git` out of `src`, so a
  nix build has no repository to ask and may have no `git` on `$PATH` either.
  `flake.nix` passes `self.shortRev or (self.dirtyShortRev or null)` into both
  `packages.beckon` and `overlays.default` — **the overlay is the one that
  matters**, since every Linux and macOS host here installs through it.
- The `rerun-if-changed` lines in `git_rev` name paths git reports rather than a
  hardcoded `.git/HEAD`, because in a worktree `.git` is a *file*, HEAD lives
  under `.git/worktrees/<name>/` and the branch ref lives in the common dir.
  Verified by measurement, not by reading: from a clean build, one
  `git commit --allow-empty` and a plain `cargo build` — no `clean`, no `touch`
  — moved the printed sha from `400b452` to `f65e6d3`.

Both CI assertions on this output match a **substring** (`-notmatch "beckon"`
on Windows; `*"$want"*` against `nix eval .#beckon.version`), so the suffix is
safe by construction. `nix eval .#beckon.version` is still the bare `0.9.4` —
`package.nix`'s `version` did not change, only the build env. A future check
that compares for EQUALITY would break.

**`-dirty` can appear, and only from nix.** `dirtyShortRev` answered
`400b452-dirty` on Nix 2.34.8 in a worktree with four modified files, and it is
passed through verbatim: nix evaluates the whole tree at one instant, so it can
honestly say the build did not come from a commit. `build.rs`'s git fallback
deliberately makes no such claim — the suffix is baked when the build script
runs and `rerun-if-changed` cannot name "any file in the tree", so a dirty flag
computed there would go stale in the one direction that matters: claiming clean
while it is not.

**This closes half a problem; do not read it as the whole one.** The other half
is that the RUNNING PROCESS may not be the image on disk — on a14 a
watchdog-started beckon ran the 0.8.0 image for three hours while `--version`,
a *fresh* process started from whatever is on disk today, said 0.9.0. No
version string can fix that, which is why the settings window's About page
compares `current_exe()`'s mtime against this process's start time instead.

**`beckon-windows` still prints its own `env!("CARGO_PKG_VERSION")`** in the
About page and `chrome.rs` — untouched, because the About page already answers
the identity question a better way. **CORRECTED 2026-08-16: this used to add
"because that crate has no build script", and it has one** —
`crates/beckon-windows/build.rs`, which stamps `BECKON_TARGET` for the About
page's `Build` row and embeds the examples' manifest. So a sha there is one
more `cargo:rustc-env=` line beside `stamp_target`, not a new build script —
and still never a reach into beckon-cli's.

## `nix build` was broken from v0.8.0 to v0.9.3

Nobody noticed for a month. `c33fcf6` inserted `pub mod settings_window;`
between `#[cfg(target_os = "windows")]` and `pub mod shell;` in
`crates/beckon-windows/src/lib.rs`, leaving `shell` ungated; `package.nix`
built the whole workspace, so every Linux/macOS `nix build` hit
``E0433: unresolved module `windows` ``. The user's Hyprland laptop sat on
0.6.0 the whole time because `nix flake update beckon` could not succeed.

Nothing in CI could see it: the build matrix passes `--exclude beckon-windows`
off Windows and `release.yml` builds `-p beckon-cli`. Two guards now exist and
they cover *different* halves — `package.nix` passes
`-p beckon-cli --bin beckon`, so **nix no longer compiles `beckon-windows` at
all** and the `nix` CI job cannot catch a future ungated `mod`; the step that
can is `the whole workspace still compiles, unexcluded`
(`cargo check --workspace --all-targets`, Linux and macOS legs) in the `build`
matrix. Do not delete one believing the other covers it — and note the trap in
that step's own history: it was written as "mirroring what nix does", which
stopped being true in the same commit range that made it load-bearing.

**It earned its keep on 2026-08-16, one commit after landing.**
`crates/beckon-windows/examples/pill_probe.rs` opened with
`#![cfg(target_os = "windows")]` — an inner attribute, so it applies to the
CRATE: off Windows the whole file disappears, `main` with it, and the example
fails **E0601** rather than compiling to a no-op. Every other probe in that
directory carries an unconditional `fn main` that dispatches into a
`#[cfg(target_os = "windows")] mod win`, which is the shape to copy. **Neither
branch could have caught it alone** — the file lived on `four-doors-phase-0`,
whose gate excludes the crate, and the step lived on `main`, where the file did
not exist — so the merge was the first tree that had both, and CI went red on
the merge commit and stayed red through the v0.9.4 tag. A local gate built from
the `--exclude` shape alone will not see this class; add a bare
`cargo check --workspace --all-targets` to it.

## Homebrew formula ships a macOS LaunchAgent

`service do` in `packaging/homebrew/beckon.rb.template`, so
`brew services start beckon` is the whole resident-mode install. Guarded by a
top-level `if OS.mac?`: `brew style` rejects a `service` block nested in
`on_macos do` (`FormulaAudit/ComponentsOrder`), and the `run macos:` form
leaves `service?` true on Linux — where `serve` does not exist — so
`brew services start` fails there instead of the formula simply having no
service.

## The packager auto-bump

The workflow needs a fine-grained PAT in repo secret `PACKAGER_TOKEN` with
`Contents: write` on `xom11/homebrew-tap` and `xom11/scoop-bucket` only.
Renewal procedure is documented in the tap repo's README. **Rotated
2026-08-11; expires 2027-08-12.**

**CORRECTED 2026-08-13: `Bump packagers` DOES fire on its own, and has since
the `workflow_call` fix landed.** `release.yml` ends with a job that does
`needs: release` and `uses: ./.github/workflows/bump-packagers.yml`, so the
bump is a step of the release rather than a reaction to it. Verified from the
bucket rather than from the workflow file: `xom11/scoop-bucket` carries
`beckon 0.9.0` at 2026-08-12T22:38Z and `beckon 0.9.1` at 2026-08-13T02:41Z,
both minutes after their tags and neither dispatched by hand. **No manual
`gh workflow run` is needed.**

This entry used to say the opposite, and the reasoning was sound for the code
at the time: `bump-packagers.yml` also listens for `release: published`, the
release is created by `release.yml` with `GITHUB_TOKEN`, and GitHub raises no
workflow events for that token — the recursion guard. That was measured at
v0.8.0, when the last `Bump packagers` run really was months old. The
`workflow_call` chain was the fix named there and it was taken; the entry was
not updated. If a release ever does go out without a bump, that path is still
the fallback:

```sh
gh workflow run "Bump packagers" -f tag=vX.Y.Z
```

**Re-running the bump does not test the token.** Both packager repos are
public, so `git clone` with a dead token still succeeds, and both push steps in
`bump-packagers.yml` `exit 0` before reaching `git push` whenever the rendered
manifest is unchanged — so a backfill of an already-published tag is green
whether the token works or not. To actually check it, ask GitHub what the token
may do:

```yaml
env: { GH_TOKEN: "${{ secrets.PACKAGER_TOKEN }}" }
run: gh api repos/xom11/homebrew-tap --jq .permissions.push   # must be true
```

At v0.8.0 the manifest genuinely changed (0.7.0 to 0.8.0), so `git push`
actually ran and the token was exercised for real rather than skipped.

A fine-grained PAT cannot even read a repo it was not granted, so a 404 there
means the repo is missing from the token's list. `gh api rate_limit --include`
also returns a `github-authentication-token-expiration` header, which is where
the expiry above came from.

## The user's nix integration

Flake-input pattern, no hand-rolled overlay.

- `~/.nix/flake.nix` — `inputs.beckon.url = "github:xom11/beckon";
  inputs.beckon.inputs.nixpkgs.follows = "nixpkgs";`
- `~/.nix/lib/mkConfigs.nix` — `mkArgs` does `args = inputs // { ... }`, which
  **spreads inputs flat at the top level of specialArgs**. So inside any host's
  `home.nix` the input is referenced directly as `beckon`, not `inputs.beckon`.

**CORRECTED 2026-08-16: `rog` is a NixOS host, not a standalone HM one, and
`zenbook-a14` is not a nix host at all.** The old list said *"Standalone HM
hosts (`mkHomeManager`, e.g. `rog`, `desktop`, `zenbook-a14`)"* and two of its
three examples were wrong. Read out of `~/.nix/flake.nix` and confirmed against
`nix flake show`:

| builder | hosts | flake output |
|---|---|---|
| `lib.mkNixos` | `x1g6`, `vm`, **`rog`** | `nixosConfigurations` |
| `lib.mkDarwin` | `macmini`, `airm3` | `darwinConfigurations` |
| `lib.mkHomeManager` | `server`, `desktop`, `minimal` | `homeConfigurations` |

`zenbook-a14` appears nowhere — "a14" is the **Windows** laptop, and a session
reading this entry can spend a while looking for its nix host.

**The cost of the error is a command that cannot work.** `nix flake show` lists
no `homeConfigurations.rog`, so `home-manager switch --flake .#rog` — the
obvious thing to reach for after `nix flake update beckon` — fails on a host
where beckon is very much installed. `mkNixos` pulls home-manager in as a NixOS
module (`inputs.home-manager.nixosModules.home-manager`, then
`home-manager.users.${username}.imports = hmModules ++
[../hosts/<device>/home.nix]`), so on `rog` the whole HM layer ships inside the
system closure and **`sudo nixos-rebuild switch --impure --flake ~/.nix#rog` is
the one command that applies it**. There is no separate HM step to run, and
running one is not merely redundant — it errors.

- **Standalone HM hosts** (`mkHomeManager` — `server`, `desktop`, `minimal`) —
  `pkgs` is constructed with
  `overlays = [ (import ../overlays) inputs.beckon.overlays.default ]`, so
  `pkgs.beckon` works without further wiring.
- **nix-darwin / NixOS hosts** (`mkDarwin`, `mkNixos` — `macmini`, `airm3`,
  `x1g6`, `vm`, `rog`) — overlay is **not** pre-baked. The host's `home.nix`
  adds it explicitly:

  ```nix
  {pkgs, beckon, ...}: {
    nixpkgs.overlays = [
      (import ../../overlays)
      beckon.overlays.default
    ];
    home.packages = [ pkgs.beckon ];
  }
  ```

- Linux/sway:
  - `~/.nix/home-manager/environments/sway/default.nix` — `home.packages`
    includes `beckon`.
  - `~/.nix/home-manager/environments/sway/sway.d/conf.d/launch-app.conf` —
    `set $focus exec beckon` (no path), bindings use Names.
- macOS/Hammerspoon:
  - `~/.nix/hosts/airm3/home.nix` — overlay + `pkgs.beckon` wired as above.
  - `~/.nix/home-manager/dotfiles/macos/hammerspoon/MySpoons/LaunchApp.spoon/init.lua`
    — beckon-backed spoon. Uses
    `hs.task.new("/etc/profiles/per-user/$USER/bin/beckon", cb, {name}):start()`.
    **Do NOT use `hs.execute(cmd, true)`** — the second arg sources the user
    login shell, which on this user's setup runs >10s and was the source of the
    original "delay" perceived from hotkey presses.
  - `…/LaunchApp.spoon/init.lua.backup` — preserved original Lua impl for
    reference.

### Bumping beckon to latest `main`

```sh
cd ~/.nix
nix flake update beckon
git commit --only flake.lock -m 'flake: bump beckon <old> -> <new>'

# NixOS (rog, x1g6, vm) — home-manager rides inside the system closure,
# so this ONE command is the whole deploy. There is no HM step to add.
sudo nixos-rebuild switch --impure --flake ~/.nix#rog

# standalone HM (server, desktop, minimal) — only these three
home-manager switch --flake .#<host>

# macOS / nix-darwin (macmini, airm3)
sudo darwin-rebuild switch --flake .#airm3 --impure
hs -c "hs.reload()"   # reload Hammerspoon to pick up spoon changes
```

**Use `git commit --only flake.lock`, not `git add` + `git commit`.** Measured
2026-08-16: several Claude sessions share `~/.nix`, and `git commit` takes the
whole INDEX, not the file just added — a peer staged three files between the
`git status` read and the commit, and 110 lines of their work landed inside a
commit whose message was about bumping this pin. `--only` ignores the index
entirely, so a race cannot widen the commit; verify with
`git show --stat HEAD` rather than trusting the commit summary.

The bump itself is all there is to it — no manual rev / hash / Cargo.lock copy.
`flake.lock` records the pinned rev for reproducibility across machines, and
since 2026-08-16 `beckon --version` prints that rev's short sha, so a machine
can be asked directly instead of by reading the lock file that built it.
