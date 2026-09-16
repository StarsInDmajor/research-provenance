{
  description = "Research Provenance Workbench — offline deterministic provenance graph CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];
      perSystem =
        {
          self',
          pkgs,
          system,
          ...
        }:
        {
          packages = {
            research-provenance = pkgs.callPackage ./default.nix { };
            default = self'.packages.research-provenance;
          };
          checks = {
            research-provenance = self'.packages.research-provenance;
          };
        };
    };
}
