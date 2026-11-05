{
  inputs = {
    cccc.url = "github:moznion/cccc/v1.6.0";
    cccc.flake = false;
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { cccc, nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      linuxSystems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      forSystems = nixpkgs.lib.genAttrs;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

      # The Herdr release that the tests run, whatever Herdr is installed.
      # .agents/wiki/herdr-tools.md says how to change it.
      herdrVersion = "0.9.5";
      herdrAssets = {
        aarch64-darwin = {
          name = "herdr-macos-aarch64";
          hash = "sha256-+3fCIyoFqANUITgsiUOS9BdOTjihjkivPdApAVfDp+I=";
        };
        aarch64-linux = {
          name = "herdr-linux-aarch64";
          hash = "sha256-F9dTby2lCFlSNoDvJ53/ysjkojX05G/o2mwuxEFBiXw=";
        };
        x86_64-linux = {
          name = "herdr-linux-x86_64";
          hash = "sha256-FNnovItKngEY13wyJm921GHwm0A0LrI3UpYL2J9Zkp0=";
        };
      };
      # The herdr-app and herdr-core (`hc`) releases that the tests run, whatever is installed:
      # Linux only, since the sandbox that runs them is bubblewrap.
      # .agents/wiki/herdr-tools.md says how to change them.
      herdrAppVersion = "0.1.13";
      herdrAppAssets = {
        aarch64-linux = "2e5bb05c47cffbad018ad34217ea4c816476d7eb842692f740798c9020f0281e";
        x86_64-linux = "77b0be72a46ca14bb367365a04164641f914b3a69712bac391d39c56a959bef4";
      };
      hcVersion = "0.1.18";
      hcAssets = {
        aarch64-linux = "cf33393cf950bb763734dc3e16610ab9daf8492391a4ef0de56026cd7f752853";
        x86_64-linux = "5b2f97cb4b2a9d7a6b2cc5942628203ed9fbb7cc0c2b4b09ad8a6f5648e246b7";
      };

      herdrFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.stdenvNoCC.mkDerivation {
          pname = "herdr";
          version = herdrVersion;
          src = pkgs.fetchurl {
            url = "https://github.com/herdrdev/herdr/releases/download/v${herdrVersion}/${herdrAssets.${system}.name}";
            inherit (herdrAssets.${system}) hash;
          };
          dontUnpack = true;
          # Keep the release binary byte for byte.
          dontFixup = true;
          installPhase = "install -Dm755 $src $out/bin/herdr";
        };
      releaseTarget = system: nixpkgs.lib.replaceStrings [ "-linux" ] [ "" ] system;
      herdrAppFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.stdenv.mkDerivation {
          pname = "herdr-app";
          version = herdrAppVersion;
          src = pkgs.fetchurl {
            url = "https://releases.herdr.dev/app/${herdrAppVersion}/herdr-app-linux-${releaseTarget system}.tar.gz";
            sha256 = herdrAppAssets.${system};
          };
          nativeBuildInputs = [ pkgs.autoPatchelfHook ];
          buildInputs = [
            pkgs.libxcb
            pkgs.libxkbcommon
            pkgs.stdenv.cc.cc.lib
          ];
          # Libraries the app opens at run time, which its ELF does not name.
          runtimeDependencies = [
            pkgs.libGL
            pkgs.vulkan-loader
            pkgs.wayland
          ];
          installPhase = "install -Dm755 herdr-app $out/bin/herdr-app";
        };
      hcFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.stdenvNoCC.mkDerivation {
          pname = "herdr-core";
          version = hcVersion;
          src = pkgs.fetchurl {
            url = "https://releases.herdr.dev/core/${hcVersion}/herdr-core-linux-${releaseTarget system}.tar.gz";
            sha256 = hcAssets.${system};
          };
          sourceRoot = ".";
          # A static binary, kept byte for byte.
          dontFixup = true;
          installPhase = "install -Dm755 herdr-core $out/bin/hc";
        };
    in
    {
      # The pinned tools, for the apps' flakes to take from here rather than pin their own.
      packages = forSystems systems (
        system:
        {
          herdr = herdrFor system;
        }
        // nixpkgs.lib.optionalAttrs (builtins.elem system linuxSystems) {
          herdr-app = herdrAppFor system;
          hc = hcFor system;
        }
      );

      # `nix flake check` runs each pinned tool and fails unless its `--version` prints its name
      # and pinned version, and nothing else.
      checks = forSystems systems (
        system:
        let
          pkgs = pkgsFor system;
          isLinux = builtins.elem system linuxSystems;
          expect = bin: expected: ''
            reported="$(${bin} --version 2>&1)"
            echo "${bin} --version: $reported"
            if [ "$reported" != "${expected}" ]; then
              echo "expected: ${expected}" >&2
              exit 1
            fi
          '';
        in
        {
          pinned-versions = pkgs.runCommand "pinned-versions" { } (
            expect "${herdrFor system}/bin/herdr" "herdr ${herdrVersion}"
            + pkgs.lib.optionalString isLinux (
              expect "${herdrAppFor system}/bin/herdr-app" "herdr-app ${herdrAppVersion}"
              + expect "${hcFor system}/bin/hc" "hc ${hcVersion}"
            )
            + "touch $out\n"
          );
        }
      );

      devShells = forSystems systems (
        system:
        let
          pkgs = pkgsFor system;
          latestRustPlatform = pkgs.makeRustPlatform {
            cargo = pkgs.rust-bin.stable.latest.default;
            rustc = pkgs.rust-bin.stable.latest.default;
          };
          rustComplexityAnalyzer = latestRustPlatform.buildRustPackage {
            pname = "cccc-cli";
            version = "1.6.0";
            src = cccc;
            cargoLock = {
              lockFile = "${cccc}/Cargo.lock";
              outputHashes."tree-sitter-kotlin-0.4.0" =
                "sha256-O9zCo8G8321cpqVp4z8USwyoLtnJReNhWs1FpYa9IVQ=";
            };
            cargoBuildFlags = [ "--package=cccc-cli" ];
            cargoTestFlags = [ "--package=cccc-cli" ];
            buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.glibc.dev ];
            "BINDGEN_EXTRA_CLANG_ARGS_${pkgs.stdenv.hostPlatform.rust.rustcTarget}" =
              pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
                "-isystem ${pkgs.glibc.dev}/include"
              );
            nativeBuildInputs = [
              pkgs.git
              pkgs.rustPlatform.bindgenHook
            ];
          };
        in
        {
          default = pkgs.mkShell (
            {
              packages = [
                # The jj the tests and the gate run, whatever jj the machine has.
                pkgs.jujutsu
                pkgs.rust-bin.stable."1.90.0".default
                pkgs.cargo-nextest
                pkgs.jq
                pkgs.typos
                rustComplexityAnalyzer
              ]
              # The app sandbox (crates/app-sandbox) runs bubblewrap, enters it with nsenter, and
              # relays a forward's connections into it with socat.
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                pkgs.bubblewrap
                pkgs.util-linux
                pkgs.socat
                # The herdr-mod-vision skill records the sandbox's window, since the root window
                # of Weston's Xwayland stays black: xdotool finds it, and ffmpeg-full grabs it
                # (plain ffmpeg has no x11grab).
                pkgs.ffmpeg-full
                pkgs.xdotool
              ];
              # Not on PATH: `herdr` there stays the user's own Herdr.
              TEST_HERDR_BIN_PATH = "${herdrFor system}/bin/herdr";
            }
            // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
              # Not on PATH either: `herdr-app` and `hc` there stay the user's.
              TEST_HERDR_APP_BIN_PATH = "${herdrAppFor system}/bin/herdr-app";
              TEST_HC_BIN_PATH = "${hcFor system}/bin/hc";
              # The private herdr-app's display: a headless Weston, which starts its Xwayland.
              TEST_WESTON_BIN_PATH = "${pkgs.weston}/bin/weston";
              # Mesa's software Vulkan driver: the private herdr-app draws on the CPU, the same on
              # every machine, and never opens the user's GPU.
              TEST_HERDR_APP_VULKAN_DRIVER = "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.uname.processor}.json";
            }
          );
        }
      );
    };
}
