# Docker registry reverse proxy for SWAG.
# No tinyauth: clients authenticate against the registry's own htpasswd.
# Layers can be GBs: no body limit, stream uploads and downloads unbuffered.
# proxy.conf already sets Upgrade/Connection and proxy timeouts — do not re-set them.
{...}: {
  flake.modules.nixos.registry-swag = {
    config,
    lib,
    ...
  }: let
    cfg = config.neo.services.registry;
  in {
    config.neo.services.registry.proxyConf = lib.mkDefault ''
      server {
        include /config/nginx/listen-https.conf;
        http2 on;
        server_name ${cfg.subdomain}.*;
        include /config/nginx/ssl.conf;
        client_max_body_size 0;
        chunked_transfer_encoding on;
        include /config/nginx/geo-access.conf;

        location / {
          include /config/nginx/proxy.conf;
          include /config/nginx/resolver.conf;
          set $upstream_app registry;
          set $upstream_port ${toString cfg.port};
          set $upstream_proto http;
          proxy_request_buffering off;
          proxy_buffering off;
          proxy_pass $upstream_proto://$upstream_app:$upstream_port;
        }
      }
    '';
  };
}
