# Shared SSH connection options (host, user, key, extra flags) for backup, remote builds, etc.
#
# Ranks are level-dependent (siblings only). Use `rankBase` so the four fields sit
# as a contiguous local band without inventing global numbers per call site:
#   host = rankBase, user = +10, sshKey = +20, extraOptions = +30
#
# Merge into a submodule:
#   options = { ... } // lib.neo.mkSshConnectionOptions {
#     rankBase = 10;  # after enabled = 0
#     hostDescription = "...";
#   };
{lib, ...}: {
  libExtensions.ssh = {
    neo = {
      # Default path of the auto-generated homeserver ed25519 key (see modules/core/base.nix).
      defaultHomeserverSshKey = "/home/homeserver/.ssh/id_ed25519";
      # Machine git identity key (see modules/core/git.nix), root:neo-git 0640.
      gitSshKey = "/var/lib/neo/git/id_ed25519";

      # `<name> [ensure|rotate]`: create an ed25519 keypair at `path` if missing
      # (`rotate` deletes it first). Shared by activation and the web UI rotate
      # button so both produce the same owner/mode. chown only runs as root.
      mkSshKeyScript = pkgs: {
        name,
        path,
        owner,
        group,
        comment,
        # Private key mode. Group-readable keys are rejected by ssh only for
        # their owner, so a shared key must be owned by someone who never uses it.
        mode ? "600",
        dirMode ? "700",
        # Fail instead of creating it (e.g. the owner's home directory).
        requireDir ? null,
      }:
        pkgs.writeShellScriptBin name ''
          set -euo pipefail
          KEY=${lib.escapeShellArg path}
          DIR=$(dirname "$KEY")

          mode="''${1:-ensure}"
          case "$mode" in
            ensure|rotate) ;;
            *)
              echo "usage: ${name} [ensure|rotate]" >&2
              exit 2
              ;;
          esac

          ${lib.optionalString (requireDir != null) ''
            if [ ! -d ${lib.escapeShellArg requireDir} ]; then
              echo ${lib.escapeShellArg "${requireDir} missing"} >&2
              exit 1
            fi
          ''}
          if [ ! -d "$DIR" ]; then
            mkdir -p "$DIR"
            chmod ${dirMode} "$DIR"
            if [ "$(id -u)" -eq 0 ]; then
              chown ${lib.escapeShellArg "${owner}:${group}"} "$DIR"
            fi
          fi

          if [ "$mode" = rotate ]; then
            rm -f "$KEY" "$KEY.pub"
          fi

          if [ ! -f "$KEY" ]; then
            ${pkgs.openssh}/bin/ssh-keygen -q -t ed25519 -N "" -f "$KEY" -C ${lib.escapeShellArg comment}
            chmod ${mode} "$KEY"
            chmod 644 "$KEY.pub"
            if [ "$(id -u)" -eq 0 ]; then
              chown ${lib.escapeShellArg "${owner}:${group}"} "$KEY" "$KEY.pub"
            fi
          fi
        '';

      mkSshConnectionOptions = {
        # Local sibling band start (host). Other SSH fields step by 10.
        rankBase ? 10,
        defaultSshKey ? "/home/homeserver/.ssh/id_ed25519",
        hostDescription ? "Remote hostname or IP",
        userDescription ? "Username for the SSH connection",
        sshKeyDescription ? "Path to the SSH private key. Defaults to the auto-generated homeserver key (created at activation if missing).",
        extraOptionsDescription ? "Additional SSH options (e.g. -o StrictHostKeyChecking=accept-new)",
      }:
        with lib; {
          host =
            mkOption {
              type = types.str;
              description = hostDescription;
            }
            // {rank = rankBase;};

          user =
            mkOption {
              type = types.str;
              description = userDescription;
            }
            // {rank = rankBase + 10;};

          sshKey =
            mkOption {
              type = types.str;
              default = defaultSshKey;
              description = sshKeyDescription;
            }
            // {rank = rankBase + 20;};

          extraOptions =
            mkOption {
              type = types.listOf types.str;
              default = [];
              description = extraOptionsDescription;
            }
            // {rank = rankBase + 30;};
        };
    };
  };
}
