{rev}: let
  flake = builtins.getFlake "git+file://${toString ../.}";
  src = builtins.fetchGit {
    url = "https://github.com/php/php-src";
    inherit rev;
    shallow = true;
  };
in
  flake.legacyPackages.${builtins.currentSystem}.phptForPhpSrc src
