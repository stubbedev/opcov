{
  perSystem = {
    cargoEnv,
    config,
    craneLib,
    lib,
    phpDefault,
    phpEnv,
    phpShell,
    pkgs,
    self',
    ...
  }: {
    devShells.default =
      (craneLib.devShell.override {
        mkShell = pkgs.mkShell.override {
          stdenv = craneLib.stdenvSelector pkgs;
        };
      }) {
        checks = self'.checks;

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
          phpShell
          rustPlatform.bindgenHook
        ];

        env = cargoEnv // phpEnv phpDefault // {RUSTC_WRAPPER = lib.getExe pkgs.sccache;};
      };
  };
}
