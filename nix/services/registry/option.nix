# Docker registry (CNCF Distribution) service options.
# Auth is the registry's own htpasswd (bcrypt) basic auth, not tinyauth:
# docker/podman clients answer a WWW-Authenticate challenge and cannot follow
# a tinyauth 302, so edge auth is unavailable for this service.
{...}: {
  flake.modules.nixos.registry-option = {
    config,
    lib,
    ...
  }:
    with lib;
    with {inherit (lib.neo) mkOption mkEnableOption;}; {
      options.neo.services.registry = mkOption {
        type = types.submodule {
          options =
            {
              enabled = mkEnableOption "Docker registry service" {rank = 0;};
              port = mkOption {
                type = types.port;
                internal = true;
                default = 5000;
                description = "Internal port the registry listens on";
              };
              users = mkOption {
                type = types.listOf types.str;
                default = [];
                rank = 10;
                description = ''
                  Registry users in username:bcrypt_hash format (htpasswd -nbB). Use the helper to add one.
                  When set, every request (push and pull) needs docker login with one of these users.
                  When empty, the registry is open: anyone who can reach it can pull and push.
                '';
                helper = lib.neo.helpers.bcryptUser;
              };
              deleteEnabled = mkOption {
                type = types.bool;
                default = false;
                rank = 20;
                description = ''
                  Allow deleting manifests and blobs by digest through the registry API.
                  Deleting only unlinks data; disk space is freed by running garbage-collect.
                '';
                ui = lib.neo.ui.mkUi {label = "Allow deletes";};
              };
            }
            // lib.neo.mkReverseProxyOptions {
              subdomain = "registry";
              auth.available = false;
            }
            // lib.neo.mkContainerDefinitions {
              registry = "registry:3";
            }
            // lib.neo.mkAppdata "${config.neo.core.volumes.appdata}/registry"
            // lib.neo.mkServiceMeta {
              category = "Files";
              icon = "https://raw.githubusercontent.com/distribution/distribution/main/docs/static/favicon.ico";
              description = ''
                A private Docker/OCI image registry based on CNCF Distribution, the reference implementation behind Docker Hub.
                Push your own images with docker push <subdomain>.<domain>/name:tag and pull them from any machine or from neo services.
                Add users to require docker login (htpasswd with bcrypt). With no users the registry is open, so restrict ingress or add a user before exposing it to the internet.
                Image data lives in appdata; enable deletes and run garbage-collect to reclaim space.
              '';
              projectUrl = "https://distribution.github.io/distribution/";
              githubUrl = "https://github.com/distribution/distribution";
              releaseUrl = "https://github.com/distribution/distribution/releases";
            }
            // lib.neo.mkSkillOptions {};
        };
        default = {};
        description = "Docker registry configuration";
      };
    };
}
