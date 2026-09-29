{neoFlake}: let
  inherit (import ./extract_lib.nix {inherit neoFlake;}) neoConfig safeList sortByRank theme;
  services = neoConfig.services or {};
  swag = services.swag or {};
  domain = swag.domain or null;
  hostname = neoConfig.core.hostname or "nixos";

  isProxied = n: let
    v = services.${n} or {};
  in
    # Include swag when it has a dashboard subdomain (sidebar + navigator).
    (v.enabled or false) && (v.subdomain or null) != null;

  proxiedNames = builtins.filter isProxied (builtins.attrNames services);

  raw =
    map (n: let
      svc = services.${n};
      meta = svc.meta or {};
    in {
      name = n;
      # Runtime units for the sidebar status dot.
      units = safeList (svc.systemdUnits or []);
      timers = safeList (svc.systemdTimers or []);
      subdomain = svc.subdomain or "";
      icon = meta.icon or null;
      rank = meta.rank or null;
      iframeCompatible = meta.iframeCompatible or true;
    })
    proxiedNames;
in {
  inherit domain hostname theme;
  services = sortByRank raw;
}
