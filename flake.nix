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

        runCiLocally = pkgs.writeScriptBin "ci-local" ''
          echo "Checking Rust formatting..."
          cargo fmt --check

          echo "Checking clippy..."
          cargo clippy --all-targets

          echo "Checking spelling..."
          codespell \
            --skip target,.git \
            --ignore-words-list crate

          echo "Testing Rust code..."
          cargo test

          echo "Generating code coverage..."
          cargo llvm-cov --workspace --lcov --output-path lcov.info
        '';

        nativeBuildInputs = with pkgs; [ rustPlatform.bindgenHook pkg-config ];
        buildInputs = [ runCiLocally ] ++ (with pkgs; [
          # Rust stuff, some stuff dev-only
          (rust-bin.nightly.latest.default.override {
              extensions = ["rust-src" "rust-analyzer" "llvm-tools-preview"];
            })
            cargo-llvm-cov

            # The C-library itself
          psrdada.packages.${system}.default

          # Linting support
          codespell
        ]);
      in with pkgs; {
        devShells.default = mkShell { inherit buildInputs nativeBuildInputs; };
      });
}
