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
      devShell = pkgs.mkShell {
        packages = [
          pkgs.rustup
        ];
        nativeBuildInputs = with pkgs; with pkgs.xorg; [
          libxcb
          libXcursor
          libXrandr
          libXi
          pkg-config
        ];
        buildInputs = with pkgs; [
          xorg.libX11
          wayland
          libxkbcommon
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.libGL
          pkgs.libxkbcommon
        ];
      };
    }
  );
}

