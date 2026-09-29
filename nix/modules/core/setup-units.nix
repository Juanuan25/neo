# Every unit built with lib.neo.mkSetupService must be declared in the
# systemdUnits of an enabled Neo service. Otherwise the web UI never shows it
# and neo-<service>.target does not restart it with its service.
{...}: {
  flake.modules.nixos.setup-units = {
    config,
    lib,
    ...
  }: let
    # Same guard as service-targets.nix: ntp/vpn-style services may lack the option.
    listOr = svc: name: let
      got = builtins.tryEval (svc.${name} or []);
    in
      if got.success && builtins.isList got.value
      then got.value
      else [];

    declared = lib.concatLists (lib.mapAttrsToList (_: svc: listOr svc "systemdUnits")
      (lib.filterAttrs (_: svc: svc.enabled or false) (config.neo.services or {})));

    setupUnits = lib.attrNames (lib.filterAttrs (_: lib.neo.isSetupService) config.systemd.services);
    undeclared = lib.filter (u: !(builtins.elem u declared)) setupUnits;
  in {
    assertions = [
      {
        assertion = undeclared == [];
        message = ''
          Setup units built with lib.neo.mkSetupService are not listed in any enabled service's systemdUnits:
            ${lib.concatStringsSep ", " undeclared}
          Add them to mkContainerDefinitions { extraUnits = [ ... ]; } or lib.neo.mkSystemdUnits in the owning option.nix.
        '';
      }
    ];
  };
}
