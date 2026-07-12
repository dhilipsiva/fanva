{
  description = "fanva — agentic English→Lojban translator: reproducible dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Pinned Rust toolchain (edition 2024 needs >=1.85; `stable.latest`
        # is well past that). Includes the wasm target the Dioxus UI compiles to.
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          targets = [ "wasm32-unknown-unknown" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          # `rust` first so its cargo/rustc shadow any global rustup shims in ~/.cargo/bin.
          packages = [
            rust
            pkgs.dioxus-cli   # `dx` — Dioxus 0.7.x CLI (dev server / bundler)
            pkgs.wrangler     # Cloudflare Worker dev/deploy for fanva-proxy
            pkgs.nodejs       # so wrangler/npm don't depend on the Windows-mounted node
            pkgs.just         # `just fetch-dict` etc. (see TODO.md)
            pkgs.pkg-config   # for native cargo-test deps that link C libs
            pkgs.openssl      #   (e.g. openssl-sys); harmless for the wasm build
            pkgs.git
          ];

          shellHook = ''
            # Let rust-analyzer resolve std sources (goto-definition into std).
            export RUST_SRC_PATH="${rust}/lib/rustlib/src/rust/library"
            echo "fanva dev shell → $(rustc --version)"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
