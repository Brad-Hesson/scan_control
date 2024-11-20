{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = flakes: flakes.flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import flakes.nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
    in
    {
      devShell = pkgs.mkShell rec {
        packages = [
          pkgs.rustup
          pkgs.renderdoc
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
      };
    }
  );
}

