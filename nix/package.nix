{
  rustPlatform,
  lib,
}:

rustPlatform.buildRustPackage {
  pname = "beckon";
  version = "0.1.0";

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

  # No tests yet; cargo-test would also need IPC mocks. Re-enable when
  # we add unit tests for desktop.rs / state.rs.
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
