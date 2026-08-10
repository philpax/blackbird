{
  description = "blackbird — a subsonic music client";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    let
      # blackbird's system dependencies (ALSA, GTK, D-Bus, Wayland, etc.) are
      # Linux-only, so we only expose outputs for Linux systems.
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      # System libraries needed to both build and run the blackbird TUI client.
      # Several of these (Wayland, libxkbcommon) are loaded at runtime via
      # dlopen, so they must also be reachable through LD_LIBRARY_PATH for the
      # final binary. xdotool provides libxdo, which muda's menu implementation
      # links against (transitively via tray-icon), so it is required at link
      # time even though the retired GUI that directly used it is gone.
      systemDeps =
        pkgs: with pkgs; [
          alsa-lib.dev
          alsa-lib.out
          dbus.dev
          dbus.lib
          wayland
          libxkbcommon
          libGL
          gdk-pixbuf
          gtk3
          cairo
          pango
          atk
          xdotool
          glib
          libappindicator-gtk3
        ];

      # Build the blackbird binary for a given nixpkgs instance. Factored out so
      # it can back both the per-system package outputs and the overlay.
      mkBlackbird =
        pkgs:
        let
          deps = systemDeps pkgs;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "blackbird";
          version = "0.1.0";

          # In a flake, `./.` is already restricted to git-tracked files, so the
          # target and spotcheck-output directories are excluded automatically.
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            # Git dependencies are not covered by the crates.io hashes in the
            # lockfile, so their source hashes are pinned explicitly here.
            outputHashes = {
              "cpal-0.18.0" = "sha256-s7O4jeM344Gk5+/4SuQSHkSZnEqghnYL1beuHn89SK8=";
              "rodio-0.22.2" = "sha256-8eUYtpNaawlaMMf908si+h9P7E5D9qsYuKW1+QCSwZw=";
              "ratatui-image-11.0.6" = "sha256-0n+L0cEY3v2E7tFTSj4NbHL5EKtBfYuq1BkLa+6b+U0=";
            };
          };

          # .cargo/config.toml pins clang + lld as the linker for fast local
          # builds; neither is present in the build sandbox, so drop it and let
          # cargo use the standard toolchain.
          postPatch = ''
            rm -f .cargo/config.toml
          '';

          # A single-package build: the workspace's default member is the
          # blackbird TUI client.
          cargoBuildFlags = [
            "-p"
            "blackbird"
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
            makeWrapper
          ];
          buildInputs = deps;

          # The test suite needs audio and display devices that are unavailable
          # in the build sandbox.
          doCheck = false;

          # Expose the dlopen-ed libraries to the binary at runtime.
          postInstall = ''
            for bin in blackbird; do
              wrapProgram $out/bin/$bin \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath deps}
            done
          '';

          meta = {
            description = "A subsonic music client (TUI)";
            homepage = "https://github.com/philpax/blackbird";
            mainProgram = "blackbird";
            platforms = supportedSystems;
          };
        };
    in
    flake-utils.lib.eachSystem supportedSystems (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        deps = systemDeps pkgs;
      in
      {
        packages.default = mkBlackbird pkgs;
        packages.blackbird = mkBlackbird pkgs;

        # Development shell: provides the system dependencies and the runtime
        # library path, but deliberately does not build the binary (bring your
        # own Rust toolchain, e.g. via rustup).
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = deps;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath deps;
        };
      }
    )
    // {
      # Overlay for including blackbird in a wider Nix configuration.
      overlays.default = final: _prev: {
        blackbird = mkBlackbird final;
      };
    };
}
