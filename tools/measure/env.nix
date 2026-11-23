{php ? "php85"}: let
  lock = (builtins.fromJSON (builtins.readFile ../../flake.lock)).nodes.nixpkgs.locked;
  pkgs = import (builtins.fetchTree lock) {};
  inherit (pkgs) lib;
  base = pkgs.${php};
  extensions = pkgs."${php}Extensions";
  turbocov = base.buildPecl {
    pname = "turbocov";
    version = "0.1.1";
    src = pkgs.fetchFromGitHub {
      owner = "shipmonk-rnd";
      repo = "turbocov";
      rev = "6ba58e483b370c07d9170fa68a17a6b2be36e56c";
      hash = "sha256-INtCsdxL4GBBnoXh5Myb+9aY/uRPapgak27H5XlDoJQ=";
    };
    configureFlags = ["--enable-turbocov"];
    nativeBuildInputs = [pkgs.pkg-config];
    buildInputs = [pkgs.pcre2];
    doCheck = false;
    meta.broken = lib.versionAtLeast base.version "8.6";
  };
  drivers = {
    pcov = extensions.pcov;
    xdebug = extensions.xdebug;
    inherit turbocov;
  };
  buildable = lib.filterAttrs (_: drv: !(drv.meta.broken or false)) drivers;
in
  pkgs.linkFarm "opcov-measure-${php}" (
    [
      {
        name = "bin/php";
        path = lib.getExe base;
      }
      {
        name = "bin/php-config";
        path = "${base.unwrapped.dev}/bin/php-config";
      }
      {
        name = "bin/composer";
        path = lib.getExe base.packages.composer;
      }
      {
        name = "bin/time";
        path = lib.getExe pkgs.time;
      }
      {
        name = "bin/git";
        path = lib.getExe pkgs.gitMinimal;
      }
      {
        name = "bin/timeout";
        path = "${pkgs.coreutils}/bin/timeout";
      }
      {
        name = "bin/perf";
        path = lib.getExe pkgs.perf;
      }
    ]
    ++ lib.mapAttrsToList (name: drv: {
      name = "ext/${name}.so";
      path = "${drv}/lib/php/extensions/${name}.so";
    })
    buildable
  )
