{
  description = "fanva — agentic English→Lojban translator: reproducible dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    # Reference Lojban parser (camxes PEG, Node.js) for the gerna
    # parse-differential (`just verify-parser`), and the vendor-refresh source
    # for fanva-ui's browser camxes. Pinned to the SAME rev as the vendored
    # assets — one grammar source of truth (see
    # fanva-ui/assets/js/vendor/camxes/VENDOR.md).
    ilmentufa = {
      url = "github:lojban/ilmentufa/778ea138f7d150121ca722db7536ce3b123943ac";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ilmentufa }:
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

        # libFuzzer needs nightly sanitizer-coverage flags (`just fuzz-parse`);
        # stable stays the shell default — the nightly is PATH-prefixed only
        # inside the fuzz recipes via NIBLI_NIGHTLY_BIN.
        nightlyFuzz = pkgs.rust-bin.nightly."2026-03-15".default;
      in
      {
        devShells.default = pkgs.mkShell {
          # `rust` first so its cargo/rustc shadow any global rustup shims in ~/.cargo/bin.
          packages = [
            rust
            pkgs.dioxus-cli   # `dx` — Dioxus 0.7.x CLI (dev server / bundler)
            pkgs.wrangler     # Cloudflare Worker dev/deploy for fanva-proxy
            pkgs.nodejs       # wrangler/npm + runs the ilmentufa camxes CLI (verify-parser)
            pkgs.just         # recipe runner (see Justfile)
            pkgs.wasm-pack    # `just test-fanva-wasm` (wasm-bindgen tests under node)
            pkgs.binaryen     # wasm-opt — `dx build --release` optimizes the bundle with it
            pkgs.cargo-fuzz   # `just fuzz-parse` (with the pinned nightly below)
            pkgs.python3      # the Lojban flywheel (python/) + `just fuzz-seed`
            pkgs.pkg-config   # for native cargo-test deps that link C libs
            pkgs.openssl      #   (e.g. openssl-sys); harmless for the wasm build
            pkgs.git
          ];

          # The pinned ilmentufa checkout (camxes.js + CLI wrappers) for the
          # parse-differential; consumed by fanva-verify's parser_diff harness.
          # (Env var names keep the NIBLI_ prefix — the harness code reads them.)
          NIBLI_CAMXES_DIR = "${ilmentufa}";
          # Nightly toolchain bin dir for cargo-fuzz (sanitizer-coverage flags).
          NIBLI_NIGHTLY_BIN = "${nightlyFuzz}/bin";

          shellHook = ''
            # Let rust-analyzer resolve std sources (goto-definition into std).
            export RUST_SRC_PATH="${rust}/lib/rustlib/src/rust/library"
            echo "fanva dev shell → $(rustc --version)"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
