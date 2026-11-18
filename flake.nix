{
  description = "rava — a Java virtual machine in Rust, hosting the reference class library";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      # Same version as rust-toolchain.toml, so rustup (outside the shell), this
      # devShell and CI agree: a mismatch rebuilds `target/` on every switch and can
      # make rustfmt reach different verdicts.
      rust = pkgs.rust-bin.stable."1.100.0".default.override {
        extensions = ["clippy" "rustfmt" "rust-src" "rust-analyzer"];
      };
      # The hosted class library is JDK 21's: class-file version 65, and the module image
      # the VM reads classes from. There is no second JDK to pin.
      jdk21 = pkgs.jdk21;
    in {
      devShells.default = pkgs.mkShell {
        nativeBuildInputs = [rust jdk21];

        shellHook = ''
          export JAVA21_HOME="${jdk21}"
          export JAVA_HOME="$JAVA21_HOME"
          export PATH="$JAVA21_HOME/bin:$PATH"
          export RUST_BACKTRACE=1
        '';
      };
    });
}
