{ fenix, pkgs }:

let
  # rust-gpu's codegen backend is coupled to the exact nightly declared here.
  # The hash only verifies Fenix's download. If the toolchain changes, Nix
  # reports the replacement hash to use.
  rustGpuToolchain = fenix.fromToolchainFile {
    file = ./shader-builder/rust-toolchain.toml;
    sha256 = "1w0vqkm9mxl6ilfpal3lj78azamghsdwzbs4bh8c13wpq1bqmx9n";
  };
in
{
  buildEnv.RUST_GPU_TOOLCHAIN_BIN = "${rustGpuToolchain}/bin";

  runtimeDeps = [
    # rustc loads rust-gpu's codegen backend dynamically during builds.
    pkgs.stdenv.cc.cc.lib
  ];
}
