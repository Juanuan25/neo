# Authentik service implementation (server, worker, Postgres).
# No tinyauth in front: Authentik is the identity provider, its flows and OIDC/SAML endpoints are public.
# WebSockets (admin UI, outposts) use SWAG proxy.conf Upgrade/Connection. Do not re-set them.
# The worker gets no Docker socket: outposts are deployed manually, not managed by Authentik.
{...}: {
  flake.modules.nixos.authentik = {
    config,
    lib,
    ...
  }:
    with lib; let
      cfg = config.neo.services.authentik;
      appdata = "${config.neo.core.volumes.appdata}/authentik";
      domain = config.neo.services.swag.domain or null;
      # Public origin for absolute links (emails, embedded outpost, OIDC issuer hints).
      publicUrl =
        if (cfg.customDomains or []) != []
        then "https://${builtins.head cfg.customDomains}"
        else if cfg.subdomain != null && domain != null && domain != ""
        then "https://${cfg.subdomain}.${domain}"
        else null;
      secretSet = v: v != null && v != "";
      dbUser = "authentik";
      dbName = "authentik";
      dbPassword =
        if secretSet cfg.dbPassword
        then cfg.dbPassword
        else "";
      # Image default user is authentik (1000). Use the neo uid/gid so appdata ownership matches.
      runAs = "--user=${toString config.neo.core.uid}:${toString config.neo.core.gid}";
      # Shared by server and worker. Bootstrap values are only read on an empty database.
      commonEnv =
        {
          AUTHENTIK_SECRET_KEY = cfg.secretKey or "";
          AUTHENTIK_POSTGRESQL__HOST = "authentik-db";
          AUTHENTIK_POSTGRESQL__NAME = dbName;
          AUTHENTIK_POSTGRESQL__USER = dbUser;
          AUTHENTIK_POSTGRESQL__PASSWORD = dbPassword;
        }
        // optionalAttrs (publicUrl != null) {
          AUTHENTIK_WEB__BASE_URL = publicUrl;
        }
        // optionalAttrs (secretSet cfg.bootstrapEmail) {
          AUTHENTIK_BOOTSTRAP_EMAIL = cfg.bootstrapEmail;
        }
        // optionalAttrs (secretSet cfg.bootstrapPassword) {
          AUTHENTIK_BOOTSTRAP_PASSWORD = cfg.bootstrapPassword;
        };
    in {
      config = mkIf cfg.enabled {
        assertions = [
          {
            assertion = secretSet cfg.secretKey && (stringLength cfg.secretKey) >= 50;
            message = "neo.services.authentik: secretKey must be at least 50 characters when enabled (use the Generate helper).";
          }
          {
            assertion = secretSet cfg.dbPassword;
            message = "neo.services.authentik: dbPassword must be set when enabled.";
          }
          {
            assertion = cfg.containers.authentik == cfg.containers."authentik-worker";
            message = "neo.services.authentik: containers.authentik and containers.authentik-worker must be the same image tag.";
          }
        ];

        # Postgres 16 keeps its data directory at /var/lib/postgresql/data.
        systemd.services.docker-authentik-db.preStart = lib.neo.mkEnsureDirs config [
          "${appdata}/pgdata"
        ];
        systemd.services.docker-authentik.preStart = lib.neo.mkEnsureDirs config [
          "${appdata}/data"
          "${appdata}/templates"
        ];
        systemd.services.docker-authentik-worker.preStart = lib.neo.mkEnsureDirs config [
          "${appdata}/data"
          "${appdata}/templates"
          "${appdata}/certs"
        ];

        virtualisation.oci-containers.containers = {
          authentik-db = {
            image = cfg.containers."authentik-db";
            autoStart = true;
            environment = {
              POSTGRES_DB = dbName;
              POSTGRES_USER = dbUser;
              POSTGRES_PASSWORD = dbPassword;
            };
            volumes = [
              "${appdata}/pgdata:/var/lib/postgresql/data"
            ];
            extraOptions = [
              "--health-cmd=pg_isready -U ${dbUser} -d ${dbName}"
              "--health-interval=30s"
              "--health-timeout=5s"
              "--health-retries=5"
              "--health-start-period=20s"
            ];
            networks = ["internal"];
          };

          authentik = {
            image = cfg.containers.authentik;
            autoStart = true;
            cmd = ["server"];
            dependsOn = ["authentik-db"];
            environment = commonEnv;
            volumes = [
              "${appdata}/data:/data"
              "${appdata}/templates:/templates"
            ];
            extraOptions = [
              runAs
              "--shm-size=512m"
            ];
            networks = ["internal"];
          };

          authentik-worker = {
            image = cfg.containers."authentik-worker";
            autoStart = true;
            cmd = ["worker"];
            dependsOn = ["authentik-db"];
            environment = commonEnv;
            volumes = [
              "${appdata}/data:/data"
              "${appdata}/templates:/templates"
              "${appdata}/certs:/certs"
            ];
            extraOptions = [
              runAs
              "--shm-size=512m"
            ];
            networks = ["internal"];
          };
        };
      };
    };
}
