# Planner checks for neo-<service>.target membership.
{...}: {
  perSystem = {
    pkgs,
    lib,
    ...
  }: let
    plan = (import ./service-targets.nix {inherit lib;}).libExtensions.service-targets.neo.planServiceTargets;

    expectEq = name: actual: expected: let
      ok = actual == expected;
    in
      if ok
      then ""
      else ''
        echo "FAIL ${name}" >&2
        echo "  expected: ${lib.generators.toPretty {} expected}" >&2
        echo "  actual:   ${lib.generators.toPretty {} actual}" >&2
        fail=1
      '';

    fixture = plan {
      services = [
        {
          service = "immich";
          units = [
            "docker-immich-server"
            "docker-immich-machine-learning"
            "docker-immich-redis"
            "docker-immich-database"
          ];
          timers = [];
        }
        {
          service = "backup";
          units = ["backup"];
          timers = ["backup"];
        }
        {
          service = "swag";
          units = ["docker-swag" "swag-cert-reloader" "swag-patcher"];
          timers = [];
        }
        {
          service = "tailscale";
          units = ["tailscale-up" "tailscaled" "tailscale-split-dns" "dnsmasq"];
          timers = [];
        }
        {
          service = "neo";
          units = ["neo-web" "neo-bootstrap"];
          timers = [];
        }
        {
          service = "pihole";
          units = ["docker-pihole" "pihole-update-gravity"];
          timers = ["pihole-update-gravity"];
        }
        {
          service = "system-updater";
          units = ["neo-auto-update"];
          timers = ["neo-auto-update"];
        }
        {
          service = "ntp";
          units = [];
          timers = [];
        }
      ];
    };

    body = lib.concatStringsSep "\n" [
      (expectEq "immich/wants" fixture.targets.neo-immich.wants [
        "docker-immich-database.service"
        "docker-immich-machine-learning.service"
        "docker-immich-redis.service"
        "docker-immich-server.service"
      ])
      (expectEq "immich/partOf" fixture.partOf.docker-immich-server ["neo-immich.target"])
      (expectEq "immich/no-timer-stop" (fixture.timerStopFrom ? docker-immich-server) false)
      (expectEq "backup/wants-timer-only" fixture.targets.neo-backup.wants ["backup.timer"])
      (expectEq "backup/before-timer" fixture.targets.neo-backup.before ["backup.timer"])
      (expectEq "backup/job-not-partOf" (fixture.partOf ? backup) false)
      (expectEq "backup/stop-only" fixture.timerStopFrom.backup ["neo-backup.target"])
      (expectEq "swag/patcher-not-a-member" (fixture.partOf ? swag-patcher) false)
      (expectEq "swag/patcher-not-wanted" (
          builtins.elem "swag-patcher.service" fixture.targets.neo-swag.wants
        )
        false)
      (expectEq "swag/container-and-reloader" fixture.targets.neo-swag.wants [
        "docker-swag.service"
        "swag-cert-reloader.service"
      ])
      (expectEq "tailscale/no-dnsmasq" (fixture.partOf ? dnsmasq) false)
      (expectEq "tailscale/daemon" fixture.partOf.tailscaled ["neo-tailscale.target"])
      (expectEq "neo-web/no-target" (fixture.targets ? neo-neo) false)
      (expectEq "neo/bootstrap-not-a-member" (fixture.partOf ? neo-bootstrap) false)
      (expectEq "pihole/mixed" fixture.targets.neo-pihole.wants [
        "docker-pihole.service"
        "pihole-update-gravity.timer"
      ])
      (expectEq "pihole/gravity-not-restarted" (fixture.partOf ? pihole-update-gravity) false)
      (expectEq "pihole/gravity-stop-only" fixture.timerStopFrom.pihole-update-gravity ["neo-pihole.target"])
      (expectEq "updater/no-bootstrap" (
          builtins.elem "neo-bootstrap.service" fixture.targets.neo-system-updater.wants
        )
        false)
      (expectEq "updater/job-not-restarted" (fixture.partOf ? neo-auto-update) false)
      (expectEq "updater/timer-armed" (
          builtins.elem "neo-auto-update.timer" fixture.targets.neo-system-updater.wants
        )
        true)
      (expectEq "ntp/no-target" (fixture.targets ? neo-ntp) false)
      (expectEq "boot" fixture.targets.neo-immich.wantedBy ["multi-user.target"])
    ];
  in {
    checks.service-targets = pkgs.runCommand "service-targets" {} ''
      set -euo pipefail
      fail=0
      ${body}
      if [ "$fail" -ne 0 ]; then
        exit 1
      fi
      touch "$out"
    '';
  };
}
