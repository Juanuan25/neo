# Dawarich reverse proxy for SWAG.
# UI protected by tinyauth. /api/ bypasses via publicPaths (trackers, health).
# Google Takeout imports are large. mkSubdomainProxyConf's default body size is unlimited.
# proxy.conf already sets Upgrade/Connection for /cable. Do not re-set them.
{...}: {
  flake.modules.nixos.dawarich-swag = {
    config,
    lib,
    ...
  }: let
    cfg = config.neo.services.dawarich;
  in {
    config.neo.services.dawarich.proxyConf = lib.mkDefault (lib.neo.mkSubdomainProxyConf {
      inherit config cfg;
      upstream = "dawarich";
      port = cfg.port;
    });
  };
}
