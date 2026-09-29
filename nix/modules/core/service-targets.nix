# neo-<service>.target for every enabled service that declares systemd units.
# Membership rules live in lib.neo.planServiceTargets.
{...}: {
  flake.modules.nixos.service-targets = {
    config,
    lib,
    ...
  }: let
    # A service that never called the unit helpers (config-only services,
    # older plugins) has no
    # systemdUnits option. The module system throws on an undeclared option;
    # `or` does not catch that.
    listOr = svc: name: let
      got = builtins.tryEval (svc.${name} or []);
    in
      if got.success && builtins.isList got.value
      then got.value
      else [];

    services = lib.mapAttrsToList (service: svc: {
      inherit service;
      units = listOr svc "systemdUnits";
      timers = listOr svc "systemdTimers";
    }) (lib.filterAttrs (_: svc: svc.enabled or false) (config.neo.services or {}));

    plan = lib.neo.planServiceTargets {inherit services;};
  in {
    systemd.targets = plan.targets;

    systemd.services = lib.mkMerge [
      (lib.mapAttrs (_: partOf: {inherit partOf;}) plan.partOf)
      # The job unit, not a PartOf member: stop reaches it, start and restart do not.
      (lib.mapAttrs (_: stopFrom: {
          unitConfig.StopPropagatedFrom = stopFrom;
        })
        plan.timerStopFrom)
    ];

    # Persistent=false: arming the timer on target start must not catch up a
    # run that elapsed while the target was stopped.
    systemd.timers =
      lib.mapAttrs (_: stopFrom: {
        unitConfig.StopPropagatedFrom = stopFrom;
        timerConfig.Persistent = lib.mkForce false;
      })
      plan.timerStopFrom;
  };
}
