{
  perSystem = {
    config,
    lib,
    opcov,
    phpAsan,
    phpBuilds,
    phpDebug,
    phpShell,
    phpVmBuilds,
    pkgs,
    self',
    ...
  }: let
    inherit (pkgs.stdenv) hostPlatform;

    shell = {
      php,
      checks ? {},
      packages ? [],
      env ? {},
    }:
      (opcov.craneLib.devShell.override {
        mkShell = pkgs.mkShell.override {
          stdenv = opcov.craneLib.stdenvSelector pkgs;
        };
      }) {
        inherit checks;

        packages = with pkgs;
          [
            config.treefmt.build.wrapper
            cargo-deny
            cargo-fuzz
            cargo-nextest
            cargo-shear
            sccache
            bacon
            just
            jq
            git
            gh
            mago
            rustPlatform.bindgenHook
          ]
          ++ packages;

        env = opcov.cargoEnv // opcov.phptEnv php // {RUSTC_WRAPPER = lib.getExe pkgs.sccache;} // env;
      };

    shellFor = php:
      shell {
        inherit php;
        inherit (self') checks;
        packages = [php php.packages.composer];
      };
  in {
    devShells =
      {default = shellFor phpShell;}
      // lib.mapAttrs (_: shellFor) (phpBuilds // phpVmBuilds)
      // lib.optionalAttrs (hostPlatform.isLinux && hostPlatform.isx86_64) {
        php85-debug = shell {
          php = phpDebug;
          packages = [phpDebug.unwrapped pkgs.valgrind];
        };
        php85-asan = shell {
          php = phpAsan;
          packages = [phpAsan.unwrapped];
          env = {
            CARGO_BUILD_TARGET = hostPlatform.rust.rustcTarget;
            CARGO_UNSTABLE_BUILD_STD = "std";
            CARGO_PROFILE_DEV_CODEGEN_BACKEND = "llvm";
            RUSTFLAGS = "-Zsanitizer=address ${opcov.cargoEnv.CARGO_BUILD_RUSTFLAGS}";
            CFLAGS = "-fsanitize=address,undefined -fno-sanitize=object-size -fno-sanitize-recover=undefined -fno-omit-frame-pointer";
            NIX_CFLAGS_COMPILE = "-DZEND_TRACK_ARENA_ALLOC";
            USE_ZEND_ALLOC = "0";
            ZEND_DONT_UNLOAD_MODULES = "1";
            ASAN_OPTIONS = "detect_leaks=1:abort_on_error=1";
            UBSAN_OPTIONS = "print_stacktrace=1:halt_on_error=1";
            TEST_PHP_ARGS = "--asan";
          };
        };
      };
  };
}
