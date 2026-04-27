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
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = rec {
          beckon = pkgs.callPackage ./nix/package.nix { };
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
        beckon = final.callPackage ./nix/package.nix { };
      };
    };
}
