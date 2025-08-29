{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    brad-utils = {
      url = "github:Brad-Hesson/brad-utils";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
      inputs.crane.follows = "crane";
      inputs.fenix.follows = "fenix";
    };
    wgsl-analyzer = {
      url = "github:wgsl-analyzer/wgsl-analyzer/nightly";
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
      brad-utils = flakes.brad-utils.lib.${system};
      fenix = flakes.fenix.packages.${system};
      crane = (flakes.crane.mkLib pkgs).overrideToolchain (fenix.combine [
        fenix.stable.defaultToolchain
        fenix.stable.rust-src
      ]);
      runtimeDeps = with pkgs; [
        libxkbcommon
        wayland
        xorg.libX11
        xorg.libXcursor
        xorg.libXrandr
        xorg.libXi
        alsa-lib
        vulkan-loader
        libGL
        libGLU
      ];
      crateArgs = {
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
      #wgsl-analyzer = flakes.wgsl-analyzer.packages.${system}.default;
    in
    {
      packages.default = crate;
      apps.default = (flakes.flake-utils.lib.mkApp { drv = crate; }) // {
        meta.description = "Control software for atomic stm lithography";
      };
      devShell = crane.devShell {
        inputsFrom = [ crate ];
        packages = [ pkgs.renderdoc ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeDeps;
        RUST_LOG = "image_compute=trace,error";
        shellHook = ''
          ${brad-utils.vscodeDefaultHook}
        '';
        # ${brad-utils.vscodeSettingHook} '"${wgsl-analyzer}/bin/wgsl_analyzer"' "wgsl-analyzer\.server\.path"
      };
    }
  );
}

