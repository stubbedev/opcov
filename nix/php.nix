{
  perSystem = {
    lib,
    pkgs,
    ...
  }: let
    zts = php: (php.override {ztsSupport = true;}).unwrapped;
    clang = php: (php.override {stdenv = pkgs.clangStdenv;}).unwrapped;
    phpShell = pkgs.php85;
  in {
    _module.args = {
      inherit phpShell;
      phpDefault = phpShell.unwrapped;
      phpBuilds = {
        php84 = pkgs.php84.unwrapped;
        php84-zts = zts pkgs.php84;
        php85 = pkgs.php85.unwrapped;
        php85-zts = zts pkgs.php85;
        php86 = pkgs.php86.unwrapped;
        php86-zts = zts pkgs.php86;
      };
      phpVmBuilds = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
        php84-call-vm = clang pkgs.php84;
        php85-tailcall-vm = clang pkgs.php85;
      };
      phpEnv = php: {
        PHP_CONFIG = "${lib.getDev php}/bin/php-config";
        PHP_RUN_TESTS = "${lib.getDev php}/lib/build/run-tests.php";
      };
    };
  };
}
