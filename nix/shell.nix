{
  perSystem = {
    config,
    lib,
    opcov,
    phpShell,
    pkgs,
    self',
    ...
  }: {
    devShells.default =
      (opcov.craneLib.devShell.override {
        mkShell = pkgs.mkShell.override {
          stdenv = opcov.craneLib.stdenvSelector pkgs;
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

        env = opcov.cargoEnv // opcov.phptEnv phpShell // {RUSTC_WRAPPER = lib.getExe pkgs.sccache;};
      };
  };
}
