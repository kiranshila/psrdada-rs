{
  description = "psrdada-rs nix flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    psrdada = {
      url = "github:kiranshila/psrdada.nix";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { nixpkgs, flake-utils, psrdada, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        nativeBuildInputs = with pkgs; [ rustPlatform.bindgenHook pkg-config ];
        buildInputs = with pkgs; [
          (rust-bin.stable.latest.default.override {
              extensions = ["rust-src" "rust-analyzer"];
            })
          psrdada.packages.${system}.default
        ];
      in with pkgs; {
        devShells.default = mkShell { inherit buildInputs nativeBuildInputs; };
      });
}
