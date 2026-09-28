# One systemd target per enabled Neo service, built from the units the service
# already declares. Callers pass the unit lists; this function does not read
# the NixOS systemd config.
#
# Normal members: the target Wants them, and they PartOf the target, so
# start / stop / restart of neo-<service>.target follows through.
#
# Timer-backed oneshots (backup, updaters, gravity, vdirsyncer): the target
# Wants the .timer. Both the timer and the job use StopPropagatedFrom, so
# stopping the target disarms the timer and stops a job that is already
# running. Start arms the timer again and does not run the job. Restart is
# not a stop, so it leaves the timer armed and does not run the job.
# BindsTo/PartOf on the job would also stop the timer where the timer is
# already PartOf the job (backup.timer).
{lib, ...}: let
  sort = lib.sort (a: b: a < b);
  uniq = lib.unique;

  # Declared for the UI, but not direct members of the service target.
  # dnsmasq: host resolver. Zone file is /run/tailscale-split-dns; leases are
  # /var/lib/dnsmasq. It does not write Neo appdata.
  # neo-web: stopping it kills the UI that sent the action.
  # neo-bootstrap: neo-web requires this oneshot. A target stop would stop
  # the config repo unit and, through that requirement, the UI. It is wanted
  # by multi-user and stays up when system-updater is disabled.
  # swag-patcher: docker-swag already Wants it and the patcher is PartOf
  # docker-swag, so the target reaches it once, through that container.
  defaultExclude = ["dnsmasq" "neo-web" "neo-bootstrap" "swag-patcher"];

  zipUnitLists = maps:
    lib.zipAttrsWith (_name: vs: sort (lib.concatLists vs)) maps;
in {
  libExtensions.service-targets.neo.planServiceTargets = {
    services,
    exclude ? defaultExclude,
  }: let
    plans = builtins.filter (p: p != null) (map (
        svc: let
          units = svc.units or [];
          timers = svc.timers or [];
          isTimer = u: builtins.elem u timers;
          isExcluded = u: builtins.elem u exclude;
          normal = sort (builtins.filter (u: !isTimer u && !isExcluded u) units);
          timerMembers = sort (
            builtins.filter (u: isTimer u && !isExcluded u) (uniq timers)
          );
          wants = sort (
            map (u: "${u}.service") normal
            ++ map (u: "${u}.timer") timerMembers
          );
          target = "neo-${svc.service}";
        in
          if wants == []
          then null
          else {
            inherit target normal timerMembers wants;
            service = svc.service;
          }
      )
      services);
  in {
    targets = lib.listToAttrs (map (p: {
        name = p.target;
        value = {
          description = "Neo ${p.service}";
          wantedBy = ["multi-user.target"];
          wants = p.wants;
          before = map (u: "${u}.timer") p.timerMembers;
        };
      })
      plans);

    # Long-running units and setup units. Restart of the target restarts these.
    partOf = zipUnitLists (map (p:
      lib.genAttrs p.normal (_: ["${p.target}.target"]))
    plans);

    # Timer and its job. StopPropagatedFrom only: target stop disarms the
    # timer and stops an in-progress job. Target start/restart does not run
    # the job, and does not restart the timer.
    timerStopFrom = zipUnitLists (map (p:
      lib.genAttrs p.timerMembers (_: ["${p.target}.target"]))
    plans);
  };
}
