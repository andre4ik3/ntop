{
  inputs.nixpkgs.url = "https://nixpkgs.flake.andre4ik3.dev";

  outputs =
    { nixpkgs, self, ... }:
    let
      inherit (nixpkgs) lib;
      eachSystem = lib.genAttrs lib.systems.flakeExposed;
    in
    {
      overlays = rec {
        default = ntop;
        ntop = final: prev: {
          ntop = final.callPackage ./default.nix { };
        };
      };

      packages = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          pkgs' = self.overlays.ntop pkgs pkgs;
        in
        rec {
          default = ntop;
          inherit (pkgs') ntop;
        }
      );

      apps = eachSystem (system: rec {
        default = ntop;
        ntop = {
          type = "app";
          program = lib.getExe self.packages.${system}.ntop;
        };
      });
    };
}
