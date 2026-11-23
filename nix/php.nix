{
  perSystem = {
    lib,
    pkgs,
    ...
  }: let
    zts = php: php.override {ztsSupport = true;};
    clang = php: php.override {stdenv = pkgs.clangStdenv;};
    debug = configureFlags: php:
      (php.override {
        packageOverrides = _: prev: {
          extensions = lib.mapAttrs (_: extension: extension.overrideAttrs {doCheck = false;}) prev.extensions;
        };
      }).overrideAttrs (prev: {
        configureFlags = prev.configureFlags ++ ["--enable-debug"] ++ configureFlags;
        dontStrip = true;
        separateDebugInfo = false;
      });
  in {
    _module.args = {
      phpZts = zts;
      phpShell = pkgs.php85;
      phpBuilds = {
        php84 = pkgs.php84;
        php84-zts = zts pkgs.php84;
        php85 = pkgs.php85;
        php85-zts = zts pkgs.php85;
        php86 = pkgs.php86;
        php86-zts = zts pkgs.php86;
      };
      phpOfficialImages = {
        php84 = "8.4-cli";
        php84-zts = "8.4-zts";
        php85 = "8.5-cli";
        php85-zts = "8.5-zts";
        php86-zts = "8.6-cli";
      };
      phpVmBuilds = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
        php84-call-vm = clang pkgs.php84;
        php85-tailcall-vm = clang pkgs.php85;
      };
      phpDebug = debug [] pkgs.php85;
      phpAsan = debug ["--enable-address-sanitizer" "--enable-undefined-sanitizer"] (clang (pkgs.php85.override {valgrindSupport = false;}));
    };
  };
}
