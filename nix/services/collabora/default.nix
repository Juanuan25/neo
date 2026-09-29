# Collabora Online service implementation (requires nextcloud).
{...}: {
  flake.modules.nixos.collabora = {
    config,
    lib,
    pkgs,
    ...
  }:
    with lib; let
      cfgNextcloud = config.neo.services.nextcloud;
      cfg = config.neo.services.collabora;
      domain = config.neo.services.swag.domain;
      nextcloudUrl = "${cfgNextcloud.subdomain}.${domain}";
      collaboraUrl = "${cfg.subdomain}.${domain}";
    in {
      config = mkIf cfg.enabled {
        assertions = [
          {
            assertion = cfgNextcloud.enabled;
            message = "neo.services.collabora: can only be enabled if neo.services.nextcloud is also enabled.";
          }
        ];

        virtualisation.oci-containers.containers.collabora = {
          image = cfg.containers.collabora;
          autoStart = true;
          # CODE 26.04+ entrypoint is coolwsd --use-env-vars, which ignores
          # extra_params. Pass SSL overrides as cmd so they append to the
          # entrypoint (reverse proxy terminates TLS; container speaks plain HTTP).
          cmd = [
            "--o:ssl.enable=false"
            "--o:ssl.termination=true"
          ];
          environment = {
            domain = nextcloudUrl;
            aliasgroup1 = "https://${nextcloudUrl}:443,https://${builtins.replaceStrings ["."] ["\\\\."] nextcloudUrl}:443";
            server_name = collaboraUrl;
            # Skip self-signed cert generation when SSL is disabled at the app layer.
            DONT_GEN_SSL_CERT = "1";
          };
          capabilities = {
            MKNOD = true;
            SYS_ADMIN = true;
          };
          networks = ["internal"];
        };

        # Non-blocking setup (lib.neo.mkSetupService): retries every 60s until
        # occ succeeds, then stays active (exited). Re-runs when a nextcloud or
        # collabora container restarts.
        systemd.services.collabora-setup = lib.neo.mkSetupService {
          inherit pkgs;
          name = "collabora-setup";
          description = "Nextcloud post-install configuration (collabora)";
          containers = ["nextcloud-db" "nextcloud-redis" "nextcloud" "collabora"];
          attachTo = ["collabora"];
          after = ["nextcloud-setup.service"];
          wants = ["nextcloud-setup.service"];
          retryInterval = 60;
          script = let
            docker = "${pkgs.docker}/bin/docker";
            occ = "${docker} exec --user www-data nextcloud php occ";
          in ''
            echo "Configuring Collabora integration..."
            ${occ} app:install richdocuments || true
            ${occ} app:disable richdocuments
            ${occ} app:disable richdocumentscode
            ${occ} app:enable richdocuments
            ${occ} config:app:set richdocuments wopi_url --value 'http://collabora:9980'
            ${occ} config:app:set richdocuments public_wopi_url --value "https://${collaboraUrl}"
            ${occ} config:app:set richdocuments wopi_callback_url --value "https://${nextcloudUrl}"
            ${occ} config:app:set richdocuments wopi_allowlist --value "0.0.0.0/0"
            ${occ} richdocuments:activate-config
            echo "Collabora setup completed."
          '';
        };
      };
    };
}
