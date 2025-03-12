{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell rec {
  nativeBuildInputs = with pkgs; [
    pkg-config
    openssl
  ];

  buildInputs = with pkgs; [
    alsa-lib.dev
    wayland
    libxkbcommon
    libGL
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
}
