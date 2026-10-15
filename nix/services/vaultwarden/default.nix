# Vaultwarden service implementation.
{...}: {
  flake.modules.nixos.vaultwarden = {
    config,
    lib,
    ...
  }:
    with lib; let
      cfg = config.neo.services.vaultwarden;
      domain = config.neo.services.swag.domain or null;
      # Public origin: WebAuthn/FIDO2, attachments and email links all derive from DOMAIN.
      publicUrl =
        if (cfg.customDomains or []) != []
        then "https://${builtins.head cfg.customDomains}"
        else if domain != null && domain != ""
        then "https://${cfg.subdomain}.${domain}"
        else null;
    in {
      config = mkIf cfg.enabled {
        systemd.services.docker-vaultwarden.preStart = lib.neo.mkEnsureDirs config [
          "${config.neo.core.volumes.appdata}/vaultwarden"
        ];

        virtualisation.oci-containers.containers.vaultwarden = {
          image = cfg.containers.vaultwarden;
          autoStart = true;
          environment =
            {
              ROCKET_ADDRESS = "0.0.0.0";
              ROCKET_PORT = toString cfg.port;
            }
            // optionalAttrs (publicUrl != null) {
              DOMAIN = publicUrl;
            }
            // optionalAttrs (cfg.adminToken != null) {
              ADMIN_TOKEN = cfg.adminToken;
            };
          volumes = [
            "${config.neo.core.volumes.appdata}/vaultwarden:/data"
          ];
          networks = ["internal"];
        };
      };
    };
}
