# Navigator dashboard data (neo.<domain> sidebar), written at build time.
#
# The sidebar only shows the running generation's proxied services, so it is a
# pure function of this config. neo-web reads the file instead of evaluating
# the flake: the dashboard loads without a nix eval, even right after a restart
# (a full eval takes minutes on slow machines). Same data and order as
# cli/src/commands/web/nix/extract_proxied_services.nix, which stays the
# fallback when the file is absent (`neo web` in development).
#
# Served from a stable /etc path so a changed service list does not change the
# neo-web unit (no restart, the warm nix repl survives activation).
{...}: {
  flake.modules.nixos.neo-navigator = {
    config,
    lib,
    pkgs,
    ...
  }: let
    cfg = config.neo.services.neo;
    services = config.neo.services;
    etcPath = "neo-web/navigator.json";

    proxiedNames =
      builtins.filter (
        n: let
          v = services.${n};
        in
          (v.enabled or false) && (v.subdomain or null) != null
      )
      (builtins.attrNames services);

    entries =
      map (n: let
        svc = services.${n};
        meta = svc.meta or {};
      in {
        name = n;
        units = svc.systemdUnits or [];
        timers = svc.systemdTimers or [];
        subdomain = svc.subdomain;
        # A path icon becomes a store path reference of the JSON file
        # (neo-web publishes it at /service-icons/…). URL strings stay as-is.
        icon = meta.icon or null;
        rank = meta.rank or null;
        iframeCompatible = meta.iframeCompatible or true;
      })
      proxiedNames;

    # Ranked first (asc), then unranked by name — matches extract_lib.nix sortByRank.
    ranked = builtins.filter (s: s.rank != null) entries;
    unranked = builtins.filter (s: s.rank == null) entries;
    sorted =
      builtins.sort (a: b: a.rank < b.rank) ranked
      ++ builtins.sort (a: b: a.name < b.name) unranked;

    navigator = pkgs.writeText "neo-navigator.json" (builtins.toJSON {
      domain = services.swag.domain or null;
      hostname = config.neo.core.hostname or "nixos";
      inherit (cfg) theme;
      services = sorted;
    });
  in {
    config = lib.mkIf cfg.enabled {
      environment.etc.${etcPath}.source = navigator;
      systemd.services.neo-web.environment.NEO_NAVIGATOR_PATH = "/etc/${etcPath}";
    };
  };
}
