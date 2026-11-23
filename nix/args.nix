{inputs, ...}: {
  perSystem = {pkgs, ...}: {
    _module.args.opcov = import ./build.nix {inherit (inputs) crane rust-overlay;} pkgs;
  };
}
