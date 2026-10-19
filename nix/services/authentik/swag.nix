# Authentik reverse proxy for SWAG.
# No tinyauth: Authentik is the identity provider (login flows, OIDC/SAML endpoints must be public).
# proxy.conf already sets Upgrade/Connection for the WebSocket channels. Do not re-set them.
{...}: {
  flake.modules.nixos.authentik-swag = {
    config,
    lib,
    ...
  }: let
    cfg = config.neo.services.authentik;
  in {
    config.neo.services.authentik.proxyConf = lib.mkDefault (lib.neo.mkSubdomainProxyConf {
      inherit config cfg;
      upstream = "authentik";
      port = cfg.port;
      auth = false;
      # Branding uploads (logos, backgrounds) and blueprint imports.
      maxBodySize = "100M";
    });
  };
}
