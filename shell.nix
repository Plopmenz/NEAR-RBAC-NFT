{
  pkgs ? import <nixpkgs> { },
}:
(pkgs.buildFHSEnv {
  name = "nix-env";
  targetPkgs =
    pkgs: with pkgs; [
      lld
      cargo
      binaryen

      nodejs
    ];
  runScript = "bash";
}).env
