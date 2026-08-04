{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    brad-utils.url = "github:Brad-Hesson/brad-utils";
    wgsl-analyzer = {
      url = "github:wgsl-analyzer/wgsl-analyzer";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
      inputs.crane.follows = "crane";
    };
  };
  outputs = flakes: flakes.flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import flakes.nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      brad-utils = flakes.brad-utils.mkLib pkgs;
      fenix = flakes.fenix.packages.${system};
      imageComputeNix = import ./crates/image-compute/rust-gpu.nix {
        inherit fenix pkgs;
      };
      crane = (flakes.crane.mkLib pkgs).overrideToolchain (fenix.combine [
        fenix.stable.defaultToolchain
        fenix.stable.rust-src
      ]);
      runtimeDeps = imageComputeNix.runtimeDeps ++ (with pkgs; [
        udev
        alsa-lib
        vulkan-loader
        libxkbcommon
        wayland
        libX11
        libXcursor
        libXrandr
        libXi
        libGL
        libGLU
      ]);
      crateArgs = imageComputeNix.buildEnv // {
        src = ./.;
        strictDeps = true;
      };
      cargoArtifacts = crane.buildDepsOnly crateArgs;
      crate = crane.buildPackage (crateArgs // {
        inherit cargoArtifacts;
        doCheck = false;
        nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
        postFixup = ''
          wrapProgram $out/bin/scan_control \
          --set LD_LIBRARY_PATH ${pkgs.lib.makeLibraryPath runtimeDeps}
        '';
      });
      wgsl-analyzer = flakes.wgsl-analyzer.packages.${system}.default;
    in
    {
      packages.default = crate;
      apps.default = (flakes.flake-utils.lib.mkApp { drv = crate; }) // {
        meta.description = "Control software for atomic stm lithography";
      };
      devShell = crane.devShell (imageComputeNix.buildEnv // {
        inputsFrom = [ crate ];
        packages = [ pkgs.renderdoc ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeDeps;
        shellHook = ''
          ${brad-utils.vscodeSettingsHook {"wgsl-analyzer.server.path" =  "${wgsl-analyzer}/bin/wgsl-analyzer";}}
        '';
      });
    }
  );
}
