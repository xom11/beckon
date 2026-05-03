{
  stdenvNoCC,
  lib,
}:

# Pure-data derivation: copies `extensions/beckon@xom11.github.io/` into
# `$out/share/gnome-shell/extensions/beckon@xom11.github.io/`. No build step,
# no dependencies — `gnome-shell` itself is the runtime, supplied by the
# system. Consumers (home-manager, NixOS modules) link this output into
# `~/.local/share/gnome-shell/extensions/<uuid>` (per-user) or
# `share/gnome-shell/extensions/<uuid>` under the system profile.
#
# `passthru.extensionUuid` matches the convention nixpkgs uses for
# `pkgs.gnomeExtensions.*`, so this package can drop straight into a list
# typed for those (e.g. `services.gnome.extensions = [ ... ];` on NixOS).

stdenvNoCC.mkDerivation {
  pname = "beckon-gnome-extension";
  version = "0.1.0";

  # The directory is named `beckon@xom11.github.io` (extension UUID
  # convention), but `@` is rejected in nix-store path names. Override the
  # store name explicitly via `builtins.path` so the import succeeds; the
  # extension UUID is preserved by the install phase below, which is what
  # GNOME Shell actually reads.
  src = builtins.path {
    path = ../extensions + "/beckon@xom11.github.io";
    name = "beckon-gnome-extension-source";
  };

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/share/gnome-shell/extensions/beckon@xom11.github.io"
    cp -r . "$out/share/gnome-shell/extensions/beckon@xom11.github.io/"
    runHook postInstall
  '';

  passthru.extensionUuid = "beckon@xom11.github.io";

  meta = {
    description = "GNOME Shell extension exposing the D-Bus surface used by the beckon CLI";
    homepage = "https://github.com/xom11/beckon";
    license = with lib.licenses; [
      mit
      asl20
    ];
    platforms = lib.platforms.linux;
  };
}
