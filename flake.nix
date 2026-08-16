{
  description = "Cross-platform focus-or-launch app switcher";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    let
      # What `beckon --version` prints after the Cargo version. `package.nix`
      # cannot work this out for itself -- it filters `.git` out of `src` --
      # so the flake, which is the only thing that knows the revision, hands
      # it over.
      #
      # Three cases and all three are normal: a clean checkout has `shortRev`;
      # a tree with uncommitted changes has `dirtyShortRev` instead and no
      # `shortRev` at all (selecting it would be an eval ERROR, not a null,
      # which is why both defaults are spelled out); and a flake copied
      # somewhere without a repository has neither, which is the `null` that
      # leaves the version bare.
      #
      # `dirtyShortRev` carries a `-dirty` suffix of its own -- measured on
      # Nix 2.34.8, `nix eval .#beckon.BECKON_GIT_REV` in a worktree with four
      # modified files answered `400b452-dirty`. That reaches `beckon
      # --version` unchanged and should: nix evaluates the whole tree at one
      # instant, so it is the only participant here that can honestly say the
      # build did not come from a commit. `build.rs`'s own git fallback
      # deliberately makes no such claim -- see `emit_version` there.
      gitRev = self.shortRev or (self.dirtyShortRev or null);
    in
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = rec {
          beckon = pkgs.callPackage ./nix/package.nix { inherit gitRev; };
          # GNOME Shell extension that beckon-cli talks to on GNOME Wayland.
          # Optional — only consume this on machines running GNOME.
          beckon-gnome-extension = pkgs.callPackage ./nix/gnome-extension.nix { };
          default = beckon;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rustfmt
            clippy
            rust-analyzer
          ];
        };

        # `nix run .#` runs `beckon` with whatever args follow.
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/beckon";
        };
      }
    )
    // {
      # Overlay other flakes / configs can add to nixpkgs.overlays.
      overlays.default = final: prev: {
        # `gitRev` is beckon's OWN revision here, not the consuming flake's --
        # `self` is this flake. That is what makes the overlay the path that
        # actually answers the question: every host in this setup installs
        # beckon through `beckon.overlays.default`, so this is where the sha
        # in `beckon --version` comes from on a real machine.
        beckon = final.callPackage ./nix/package.nix { inherit gitRev; };
        beckon-gnome-extension = final.callPackage ./nix/gnome-extension.nix { };
      };
    };
}
