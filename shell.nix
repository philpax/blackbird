{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell rec {
  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    alsa-lib.dev
    alsa-lib.out
    dbus.dev
    dbus.lib
    wayland
    libxkbcommon
    libGL
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
}
