{
  rustPlatform,
  lib,
}:

let
  cargo = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in

rustPlatform.buildRustPackage {
  pname = cargo.package.name;
  inherit (cargo.package) version;

  src = builtins.path {
    name = "source";
    path = ./.;
  };

  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = ''
      Btop for Nix
    '';
    mainProgram = "ntop";
    license = lib.licenses.mit;
  };
}
