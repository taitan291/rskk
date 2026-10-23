{
  description = "SKK input method in Rust (core + Wayland frontend)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks-nix = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      flake.overlays.default = final: _prev: {
        rskk-wayland = inputs.self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      perSystem =
        {
          config,
          pkgs,
          system,
          ...
        }:
        let
          toolchain = inputs.fenix.packages.${system}.stable.withComponents [
            "cargo"
            "clippy"
            "rust-src"
            "rustc"
            "rustfmt"
          ];
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          clippyForHooks = pkgs.writeShellScriptBin "cargo-clippy" ''
            if [ "''${1:-}" = clippy ]; then shift; fi
            cargo_args=()
            rust_args=()
            rust_args_started=false
            for arg in "$@"; do
              if [ "$arg" = "--" ]; then
                rust_args_started=true
              elif [ "$rust_args_started" = true ]; then
                rust_args+=("$arg")
              else
                cargo_args+=("$arg")
              fi
            done
            if [ "''${#rust_args[@]}" -gt 0 ]; then
              export RUSTFLAGS="''${RUSTFLAGS:+''${RUSTFLAGS} }''${rust_args[*]}"
            fi
            export RUSTC_WORKSPACE_WRAPPER="${toolchain}/bin/clippy-driver"
            exec "${toolchain}/bin/cargo" check "''${cargo_args[@]}"
          '';
          skkJisyoL = pkgs.skkDictionaries.l;
          testFonts = pkgs.makeFontsConf {
            fontDirectories = [ pkgs.noto-fonts-cjk-sans ];
          };
          nativeDeps = with pkgs; [ pkg-config ];
          buildDeps = with pkgs; [
            libxkbcommon
            wayland
            stdenv.cc.cc.lib
          ];
          source = ./.;
          lockFile = ./Cargo.lock;
        in
        {
          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              rustfmt = {
                enable = true;
                package = toolchain;
                edition = "2024";
              };
              nixfmt.enable = true;
              taplo.enable = true;
            };
          };

          pre-commit.check.enable = false;
          pre-commit.settings.hooks = {
            treefmt = {
              enable = true;
              package = config.treefmt.build.wrapper;
            };
            clippy = {
              enable = true;
              packageOverrides = {
                cargo = toolchain;
                clippy = clippyForHooks;
              };
              settings.denyWarnings = true;
            };
            check-merge-conflicts.enable = true;
            end-of-file-fixer.enable = true;
            trim-trailing-whitespace.enable = true;
            typos.enable = true;
          };

          packages.default = rustPlatform.buildRustPackage {
            pname = "rskk-wayland";
            version = "0.1.0";
            src = source;
            cargoLock.lockFile = lockFile;
            cargoBuildFlags = [
              "-p"
              "skk-wayland"
            ];
            nativeBuildInputs = nativeDeps ++ [
              pkgs.makeWrapper
              pkgs.autoPatchelfHook
            ];
            buildInputs = buildDeps;
            FONTCONFIG_FILE = testFonts;
            postInstall = ''
              wrapProgram $out/bin/rskk-wayland \
                --set-default RSKK_SYSTEM_DICT ${skkJisyoL}/share/skk/SKK-JISYO.L \
                --set FONTCONFIG_FILE ${testFonts}
            '';
          };

          apps.default = {
            type = "app";
            program = "${config.packages.default}/bin/rskk-wayland";
          };

          checks.quality = rustPlatform.buildRustPackage {
            pname = "rskk-quality";
            version = "0.1.0";
            src = source;
            cargoLock.lockFile = lockFile;
            doCheck = true;
            nativeBuildInputs = nativeDeps;
            buildInputs = buildDeps;
            FONTCONFIG_FILE = testFonts;
            postBuild = "RUSTFLAGS='-D warnings' RUSTC_WORKSPACE_WRAPPER=${toolchain}/bin/clippy-driver cargo check --offline --workspace --all-targets";
          };

          devShells.default = pkgs.mkShell {
            inputsFrom = [ config.pre-commit.devShell ];
            nativeBuildInputs = nativeDeps;
            buildInputs = buildDeps ++ [
              toolchain
              pkgs.rust-analyzer
              pkgs.cargo-nextest
              pkgs.cargo-watch
              pkgs.time
            ];
            RSKK_SYSTEM_DICT = "${skkJisyoL}/share/skk/SKK-JISYO.L";
            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildDeps;
            FONTCONFIG_FILE = testFonts;
          };
        };
    };
}
