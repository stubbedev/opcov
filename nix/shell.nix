{
  perSystem = {
    config,
    lib,
    opcov,
    phpBuilds,
    phpShell,
    phpVmBuilds,
    pkgs,
    self',
    ...
  }: let
    shellFor = php:
      (opcov.craneLib.devShell.override {
        mkShell = pkgs.mkShell.override {
          stdenv = opcov.craneLib.stdenvSelector pkgs;
        };
      }) {
        inherit (self') checks;

        packages = with pkgs; [
          config.treefmt.build.wrapper
          cargo-deny
          cargo-nextest
          cargo-shear
          sccache
          bacon
          just
          jq
          git
          gh
          mago
          php
          php.packages.composer
          rustPlatform.bindgenHook
        ];

        env = opcov.cargoEnv // opcov.phptEnv php // {RUSTC_WRAPPER = lib.getExe pkgs.sccache;};
      };
  in {
    devShells = {default = shellFor phpShell;} // lib.mapAttrs (_: shellFor) (phpBuilds // phpVmBuilds);
  };
}
