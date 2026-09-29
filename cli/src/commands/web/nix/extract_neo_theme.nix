{
  neoFlake,
  configName ? null,
}:
(import ./extract_lib.nix {inherit neoFlake configName;}).theme
