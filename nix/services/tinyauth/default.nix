# Tinyauth forward authentication service implementation.
{...}: {
  flake.modules.nixos.tinyauth = {
    config,
    lib,
    ...
  }:
    with lib; let
      cfg = config.neo.services.tinyauth;
      domain = config.neo.services.swag.domain;

      # Collect all services that have auth enabled.
      authServices = filterAttrs (n: v: v.enabled && (v.auth.enabled or false)) config.neo.services;

      # Usernames from "username:bcrypt_hash" entries (orphans in access are ignored).
      knownUsernames = map (u: head (splitString ":" u)) cfg.users;

      # Per-user ACL entries that match a configured user (safe if user deleted but access key remains).
      activeAccess =
        filterAttrs (username: _: elem username knownUsernames) (cfg.access or {});

      # Service names that exist under neo.services (enabled or not) — for typo checks.
      knownServiceNames = attrNames config.neo.services;

      # Users who may not access app `appName` under allow-by-default + USERS_BLOCK:
      # - blacklist mode: app is in block
      # - whitelist mode: allow is non-empty and app is not listed
      usersBlockedFromApp = appName:
        lib.sort (a: b: a < b) (
          lib.filter (
            username: let
              entry = activeAccess.${username};
              allow = entry.allow or [];
              block = entry.block or [];
            in
              if allow != []
              then !(elem appName allow)
              else elem appName block
          ) (attrNames activeAccess)
        );

      entra = cfg.entra;
      entraBridgePort = 8080;

      # Entra groups that grant app `appName` (deny-by-default for Microsoft users):
      # a listed group grants every app unless groupAccess narrows it.
      groupsGrantingApp = appName:
        filter (
          group: let
            entry = cfg.groupAccess.${group} or {};
            allow = entry.allow or [];
            block = entry.block or [];
          in
            if allow != []
            then elem appName allow
            else !(elem appName block)
        )
        cfg.entraGroups;

      # Tinyauth treats an empty OAUTH_GROUPS as "no restriction", so an app no
      # group grants gets a group name nobody is in.
      noGroup = "neo-no-entra-group";

      # Generate TINYAUTH_APPS_<NAME>_* env vars for services with publicPaths / domains / user ACLs.
      # Tinyauth treats `_` as nested config keys (e.g. APPS_FOO_BAR → apps.foo.bar), so
      # hyphenated service names must not become FOO_BAR — strip hyphens instead (stirling-pdf → STIRLINGPDF).
      appAclEnvVars =
        foldlAttrs (
          acc: name: svc:
            acc
            // (
              let
                appName = lib.toUpper (replaceStrings ["-"] [""] name);
                blocked = usersBlockedFromApp name;
              in
                optionalAttrs (svc.auth.publicPaths or [] != []) {
                  "TINYAUTH_APPS_${appName}_PATH_ALLOW" = "(${concatStringsSep "|" svc.auth.publicPaths})";
                }
                // optionalAttrs (svc.subdomain or null != null) {
                  "TINYAUTH_APPS_${appName}_CONFIG_DOMAIN" = "${svc.subdomain}.${domain}";
                }
                // optionalAttrs (blocked != []) {
                  "TINYAUTH_APPS_${appName}_USERS_BLOCK" = concatStringsSep "," blocked;
                }
                // optionalAttrs entra.enabled {
                  "TINYAUTH_APPS_${appName}_OAUTH_GROUPS" = let
                    granted = groupsGrantingApp name;
                  in
                    if granted == []
                    then noGroup
                    else concatStringsSep "," granted;
                }
            )
        ) {}
        authServices;

      # groupAccess entries for listed groups (orphan keys are ignored).
      activeGroupAccess =
        filterAttrs (group: _: elem group cfg.entraGroups) cfg.groupAccess;

      # Flat list of service names referenced in allow/block (for assertions).
      referencedServiceNames = lib.unique (
        concatLists (
          mapAttrsToList (
            _: entry: (entry.allow or []) ++ (entry.block or [])
          )
          (activeAccess // activeGroupAccess)
        )
      );

      mixedAllowBlock = entries:
        filter (
          key: let
            e = entries.${key};
          in
            (e.allow or []) != [] && (e.block or []) != []
        ) (attrNames entries);
      mixedAllowBlockUsers = mixedAllowBlock activeAccess;
      mixedAllowBlockGroups = mixedAllowBlock activeGroupAccess;

      entraEnv = let
        provider = "TINYAUTH_OAUTH_PROVIDERS_ENTRA";
        login = "https://login.microsoftonline.com/${entra.tenantId}/oauth2/v2.0";
      in
        {
          "${provider}_NAME" = entra.buttonName;
          "${provider}_CLIENTID" = entra.clientId;
          "${provider}_CLIENTSECRET" = entra.clientSecret;
          "${provider}_AUTHURL" = "${login}/authorize";
          "${provider}_TOKENURL" = "${login}/token";
          # Graph user + groups as flat claims (entra_userinfo.py).
          "${provider}_USERINFOURL" = "http://tinyauth-entra:${toString entraBridgePort}/userinfo";
          "${provider}_SCOPES" = concatStringsSep "," (
            ["openid" "profile" "email" "User.Read"]
            ++ optional entra.groupNames "GroupMember.Read.All"
          );
        }
        // optionalAttrs entra.autoRedirect {
          TINYAUTH_OAUTH_AUTOREDIRECT = "entra";
        };

      unknownServiceRefs =
        filter (n: !(elem n knownServiceNames)) referencedServiceNames;
    in {
      config = mkIf cfg.enabled {
        assertions = [
          {
            assertion = cfg.users != [] || entra.enabled;
            message = "neo.services.tinyauth: Configure at least one user in 'users', or enable Entra sign-in.";
          }
          {
            assertion = !entra.enabled || all (v: v != "") [entra.tenantId entra.clientId entra.clientSecret];
            message = "neo.services.tinyauth.entra: tenantId, clientId and clientSecret are required when Entra sign-in is enabled.";
          }
          {
            assertion = mixedAllowBlockGroups == [];
            message = ''
              neo.services.tinyauth.groupAccess: groups must not set both non-empty allow and block.
              Conflicting groups: ${concatStringsSep ", " mixedAllowBlockGroups}
            '';
          }
          {
            assertion = mixedAllowBlockUsers == [];
            message = ''
              neo.services.tinyauth.access: users must not set both non-empty allow and block.
              Conflicting users: ${concatStringsSep ", " mixedAllowBlockUsers}
            '';
          }
          {
            assertion = unknownServiceRefs == [];
            message = ''
              neo.services.tinyauth.access: unknown service name(s) in allow/block: ${concatStringsSep ", " unknownServiceRefs}.
              Use neo.services.* keys (e.g. immich, searxng).
            '';
          }
        ];

        systemd.services.docker-tinyauth.preStart = lib.neo.mkEnsureDirs config [
          "${config.neo.core.volumes.appdata}/tinyauth"
        ];

        virtualisation.oci-containers.containers.tinyauth = {
          image = cfg.containers.tinyauth;
          autoStart = true;
          environment =
            {
              TINYAUTH_APPURL = "https://${cfg.subdomain}.${domain}";
              TINYAUTH_AUTH_USERS = concatStringsSep "," cfg.users;
              TINYAUTH_SERVER_PORT = toString cfg.port;
              TINYAUTH_AUTH_SESSIONEXPIRY = toString cfg.sessionExpiry;
              # Default ACL policy is allow-by-default (auth.acls.policy defaults to "allow").
              TINYAUTH_ANALYTICS_ENABLED = "false";
              TINYAUTH_DATABASE_PATH = "/data/tinyauth.db";
            }
            // optionalAttrs cfg.backgroundImage {
              TINYAUTH_UI_BACKGROUNDIMAGE = "https://lipsum.app/random/1920x1080";
            }
            // optionalAttrs entra.enabled entraEnv
            // appAclEnvVars;
          volumes = [
            "${config.neo.core.volumes.appdata}/tinyauth:/data"
          ];
          networks = ["internal"];
        };

        virtualisation.oci-containers.containers.tinyauth-entra = mkIf entra.enabled {
          image = cfg.containers.tinyauth-entra;
          autoStart = true;
          cmd = ["python3" "/app/entra_userinfo.py"];
          environment.PORT = toString entraBridgePort;
          volumes = ["${./entra_userinfo.py}:/app/entra_userinfo.py:ro"];
          networks = ["internal"];
        };
      };
    };
}
