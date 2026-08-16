{
  rustPlatform,
  lib,
  # Short sha of the revision this is built from, or null when there is not
  # one to name. `flake.nix` passes `self.shortRev` (or `dirtyShortRev`);
  # a plain `callPackage` from nixpkgs, with no flake around it, gets null
  # and ships the bare Cargo version.
  #
  # It has to arrive as an argument because there is no way to derive it
  # here: `src` below deliberately filters `.git` out, so nothing in the
  # sandbox can ask git anything. Without this a nix-installed beckon --
  # which is how every Linux and macOS host in this setup installs it --
  # reports the same `0.9.4` for every rev between two releases, and
  # `nix flake update beckon` becomes unverifiable from the machine.
  gitRev ? null,
}:

let
  # The workspace manifest is the single source of truth for the version.
  # This file used to hardcode one, and it desynced immediately: it still
  # said 0.1.0 at workspace 0.9.3, so every nix-installed beckon reported a
  # version three minor releases stale. `[workspace.package] version` is
  # where the crates read theirs from too (`version = { workspace = true }`
  # in each crate's Cargo.toml), so this cannot drift from what the binary
  # prints.
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = "beckon";
  version = cargoToml.workspace.package.version;

  src = lib.cleanSourceWith {
    src = ./..;
    filter =
      path: type:
      let
        base = baseNameOf (toString path);
      in
      !(
        base == "target"
        || base == "result"
        || base == ".git"
        # Exclude the test sandbox script (not part of the package).
        || base == "test-i3-env.sh"
      );
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Read by `crates/beckon-cli/build.rs`, which appends it to what
  # `beckon --version` prints. Set only when there is a revision to name --
  # an unconditional `BECKON_GIT_REV = ""` would still be an env var, and the
  # build script would have to distinguish empty-from-absent to avoid
  # printing `0.9.4 ()`. Absent is the simpler contract.
  #
  # This does NOT change `version` above: that stays the Cargo version, so
  # `nix eval .#beckon.version` and the CI check that compares it against
  # `beckon --version` both keep working (the check matches a substring).
  env = lib.optionalAttrs (gitRev != null) { BECKON_GIT_REV = gitRev; };

  # Build the CLI, and only the CLI. Two separate reasons, both measured:
  #
  # `-p beckon-cli` keeps `beckon-windows` out of the graph. cargo pulls the
  # per-OS backend in by target cfg (see beckon-cli/Cargo.toml), so on Linux
  # and darwin the Windows crate is not a dependency of anything here. It was
  # still compiled, because the default is `--workspace` and a workspace
  # member is built whether or not it is reachable -- and it does not compile
  # off Windows: its `windows` dependency is itself `cfg(target_os =
  # "windows")`. `nix build .#beckon` failed on Linux and darwin with E0433
  # in `beckon-windows/src/shell.rs` for exactly that reason. Gating that
  # module fixed the error; this flag removes the whole class, because the
  # crate is no longer built here at all.
  #
  # `--bin beckon` is the other half. `beckon-cli` declares a second [[bin]],
  # `beckon-serve`, and cargo cannot gate a [[bin]] on target_os -- so off
  # Windows it builds a stub whose `main` prints "beckon-serve is
  # Windows-only" and exits 1 (see crates/beckon-cli/src/bin/beckon-serve.rs).
  # nixpkgs' `cargoInstallPostBuildHook` copies *every* executable at the top
  # of `target/<triple>/release`, so without this the closure would ship that
  # stub in $out/bin beside the real binary. `meta.mainProgram` below stays
  # correct either way; what changes is that $out/bin has one entry.
  cargoBuildFlags = [
    "-p"
    "beckon-cli"
    "--bin"
    "beckon"
  ];

  # Tests run in CI's `build & test` matrix, on all three OSes, with the
  # per-OS `--exclude` shape that job already carries. Enabling them here
  # would not repeat that cheaply: `checkPhase` reads `cargoTestFlags`, not
  # `cargoBuildFlags`, so a bare `doCheck = true` runs `cargo test` over the
  # whole workspace and rebuilds the very crate the flags above exclude.
  doCheck = false;

  meta = {
    description = "Cross-platform focus-or-launch app switcher (sway/i3, more later)";
    homepage = "https://github.com/xom11/beckon";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "beckon";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
