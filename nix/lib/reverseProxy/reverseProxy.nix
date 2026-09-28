# Shared reverse-proxy option types for services (subdomain, auth, custom domains).
# Ranks are level-dependent — see nix/lib/option.nix for the service band table.
{lib, ...}: let
  neoUi = (import ../ui.nix {inherit lib;}).libExtensions.ui.neo.ui;
  inherit (neoUi.groups) access;
in {
  libExtensions.reverseProxy = {
    neo = {
      mkReverseProxyOptions = {
        subdomain ? null,
        auth ? null,
        customDomains ? [],
      } @ args: let
        authAvailable = args.auth.available or true;
        defaultAuth = {
          enabled = authAvailable;
          publicPaths = [];
        };
        auth = lib.recursiveUpdate defaultAuth (builtins.removeAttrs (args.auth or {}) ["available"]);
      in
        with lib; {
          subdomain =
            mkOption {
              type = types.nullOr types.str;
              default = subdomain;
              description = "Subdomain for the service. Must be unique among enabled services and consist of lowercase a-z only";
            }
            // {
              rank = 100;
              ui = neoUi.mkUi {
                group = access;
                label = "Subdomain";
                summary = true;
              };
            };

          ingress =
            mkOption {
              type = types.listOf (types.enum ["local" "tailscale" "web"]);
              default = ["local" "tailscale" "web"];
              description = "Where this service can be opened from. Home network = your LAN, Tailscale = devices on your tailnet, Internet = public HTTPS via rathole/streamproxy. Requests from anywhere not selected are blocked (Let's Encrypt HTTP-01 on port 80 is always allowed).";
            }
            // {
              rank = 105;
              ui = neoUi.mkUi {
                group = access;
                label = "Reachable from";
                summary = true;
                choices = ["local" "tailscale" "web"];
                choiceLabels = {
                  local = "Home network";
                  tailscale = "Tailscale";
                  web = "Internet";
                };
              };
            };

          proxyConf = mkOption {
            type = types.nullOr types.str;
            internal = true;
            default = null;
            description = "Nginx proxy conf for swag";
          };

          customDomains =
            mkOption {
              type = types.listOf types.str;
              default = customDomains;
              description = "Custom domains (one domain per string, e.g. example.com or www.example.com) that should resolve to this service; automatically added to SWAG for certificates and to Pi-hole for local DNS";
            }
            // {
              rank = 130;
              ui = neoUi.mkUi {
                group = access;
                label = "Custom domains";
              };
            };

          auth =
            mkOption {
              type = types.submodule {
                options = {
                  enabled =
                    mkOption {
                      type = types.bool;
                      default = auth.enabled;
                      description = "Require a tinyauth login before anyone can open this service";
                    }
                    // {
                      rank = 0;
                      ui = neoUi.mkUi {
                        label = "Require login";
                        summary = true;
                      };
                    };
                  publicPaths =
                    mkOption {
                      type = types.listOf types.str;
                      default = auth.publicPaths;
                      description = "Regex paths that bypass tinyauth authentication (e.g. share links, health checks)";
                    }
                    // {
                      rank = 10;
                      ui = neoUi.mkUi {
                        label = "Public paths";
                        visibleWhen = "enabled";
                      };
                    };
                };
              };
              default = auth;
              internal = !authAvailable;
              description = "Tinyauth forward authentication settings";
            }
            // {
              rank = 120;
              ui = neoUi.mkUi {group = access;};
            };
        };
      # Enabled services that get a SWAG subdomain (certs, DNS, proxy-conf materialization).
      # Includes swag itself for the dashboard UI at swag.<domain>.
      # Do NOT require proxyConf here — reading it while modules define it causes infinite recursion.
      getProxiedServices = config:
        lib.filterAttrs (
          _: v: v.enabled && (v.subdomain or null) != null
        )
        config.neo.services;
    };
  };
}
