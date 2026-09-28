{
  config,
  lib,
  ...
}: let
  settingsPath = ../settings.toml;
  raw =
    if builtins.pathExists settingsPath
    then builtins.fromTOML (builtins.readFile settingsPath)
    else {};
  core = raw.core or {};
  neoCli = raw."neo-cli" or {};
  localCli = neoCli.local or {};
  serverCli = neoCli.server or {};
  # Directory of a local flake ref (git+file:, path:, or an absolute path).
  # The part after # is a flake output name, not part of the directory.
  localDir = url: let
    base =
      if lib.hasPrefix "git+file://" url
      then lib.removePrefix "git+file://" url
      else if lib.hasPrefix "git+file:" url
      then lib.removePrefix "git+file:" url
      else if lib.hasPrefix "path:" url
      then lib.removePrefix "path:" url
      else if lib.hasPrefix "/" url
      then url
      else "";
    dir = lib.head (lib.splitString "#" base);
  in
    if dir == ""
    then null
    else dir;
  localUrl = localCli.neoInput or "";
  localPath = localDir localUrl;
  # Laptop builds follow the checkout when that directory exists. On the
  # homeserver the path is absent, so the server input (GitHub by default) wins.
  # Pure eval hides paths outside this repo (pathExists is false), so neo runs
  # `nix run --impure .#write-flake` for this check. `--impure` follows `run`.
  useLocal = localPath != null && builtins.pathExists localPath;
  sharedUrl = neoCli.neoInput or "";
  sharedIsLocal = (localDir sharedUrl) != null;
  serverUrl =
    if (serverCli.neoInput or "") != ""
    then serverCli.neoInput
    else if sharedUrl != "" && !sharedIsLocal
    then sharedUrl
    else "github:madebydamo/neo";
  neoInput =
    if useLocal
    then localUrl
    else serverUrl;
  plugins = core.plugins or [];
in {
  flake-file.inputs =
    {
      nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
      neo.url = neoInput;
      neo.inputs.nixpkgs.follows = "nixpkgs";
    }
    // lib.listToAttrs (lib.imap0 (i: p: {
        name = "plugin${toString i}";
        value = {
          url = p;
          inputs.neo.follows = "neo";
          inputs.nixpkgs.follows = "nixpkgs";
          inputs.flake-file.follows = "flake-file";
        };
      })
      plugins);
}
