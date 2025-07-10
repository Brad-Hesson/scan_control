{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };
  outputs = flakes: flakes.flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import flakes.nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      craneLib = flakes.crane.mkLib pkgs;
      wgsl-analyzer = craneLib.buildPackage {
        pname = "wgsl_analyzer";
        version = "0.0.0";
        src = pkgs.fetchFromGitHub {
          owner = "wgsl-analyzer";
          repo = "wgsl-analyzer";
          rev = "v0.8.1";
          hash = "sha256-bhosTihbW89vkqp1ua0C1HGLJJdCNfRde98z4+IjkOc=";
        };
        doCheck = false;
      };
    in
    {
      devShell = pkgs.mkShell rec {
        packages = [
          pkgs.rustup
          pkgs.renderdoc
          wgsl-analyzer
        ];
        buildInputs = with pkgs; [
          # necessary for building wgpu in 3rd party packages (in most cases)
          libxkbcommon
          wayland
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
          alsa-lib
          fontconfig
          freetype
          shaderc
          directx-shader-compiler
          pkg-config
          cmake
          mold # could use any linker, needed for rustix (but mold is fast)

          libGL
          vulkan-headers
          vulkan-loader
          vulkan-tools
          vulkan-tools-lunarg
          vulkan-extension-layer
          vulkan-validation-layers # don't need them *strictly* but immensely helpful
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
        shellHook = ''
          mkdir .vscode
          touch .vscode/settings.json
          sed -i '/wgsl-analyzer\.server\.path/c\    \"wgsl-analyzer\.server\.path\": \"${wgsl-analyzer}/bin/wgsl_analyzer",' .vscode/settings.json
        '';
      };
    }
  );
}

