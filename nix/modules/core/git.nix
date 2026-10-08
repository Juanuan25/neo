# Machine git identity (neo.core.git): one SSH key, HTTPS tokens and
# user.name/email shared by every user in neo.core.git.users (homeserver for
# builds, plugin fetches and the config repo; Hermes appends itself).
#
# Root is deliberately left out: `sudo nixos-rebuild switch` only re-evaluates a
# flake.lock that homeserver already built, so every input is in the store.
#
#   /var/lib/neo/git/id_ed25519   root:neo-git 0640 (ssh rejects a group-readable
#                                 key only for its owner, and root never uses it)
#   /etc/neo/git/tokens/<host>    root:neo-git 0640, read by neo-git-credential
#   /etc/neo/git/access-tokens.conf  Nix access-tokens, !include'd by nix.conf
#                                 (unreadable for non-members: Nix skips it)
{...}: {
  flake.modules.nixos.core-git = {
    config,
    lib,
    pkgs,
    ...
  }: let
    cfg = config.neo.core.git;
    group = "neo-git";
    tokenDir = "/etc/neo/git/tokens";
    accessTokensFile = "/etc/neo/git/access-tokens.conf";

    gitSshKey = lib.neo.mkSshKeyScript pkgs {
      name = "neo-git-ssh-key";
      path = lib.neo.gitSshKey;
      owner = "root";
      inherit group;
      comment = "neo-git@${config.neo.core.hostname}";
      mode = "640";
      dirMode = "750";
    };

    # git credential helper protocol: answer `get` for hosts with a token.
    credentialHelper = pkgs.writeShellScript "neo-git-credential" ''
      [ "''${1:-}" = get ] || exit 0
      host=""
      while IFS='=' read -r key value; do
        [ -n "$key" ] || break
        if [ "$key" = host ]; then host=$value; fi
      done
      case "$host" in "" | */* | .*) exit 0 ;; esac
      token_file=${tokenDir}/$host
      [ -r "$token_file" ] || exit 0
      echo username=x-access-token
      echo "password=$(cat "$token_file")"
    '';

    # knownHosts keys are `host` or `host:port`; ssh Match sees the bare host.
    splitHost = name: let
      m = builtins.match "([^:]+):([0-9]+)" name;
    in
      if m == null
      then {
        host = name;
        known = name;
      }
      else {
        host = builtins.elemAt m 0;
        known = "[${builtins.elemAt m 0}]:${builtins.elemAt m 1}";
      };
    sshHosts = lib.unique (map (n: (splitHost n).host) (lib.attrNames cfg.knownHosts));

    # Merged with operator hosts (attrsOf default would be replaced instead).
    builtinKnownHosts = {
      "github.com" = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
      "gitlab.com" = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAfuCHKVTjquxvt6CM6tdG4SLp1Btn/nOeHHE5UOzRdf";
    };

    validHost = name: builtins.match "[A-Za-z0-9][A-Za-z0-9.-]*(:[0-9]+)?" name != null;
  in {
    neo.core.git = {
      users = ["homeserver"];
      knownHosts = lib.mapAttrs (_: lib.mkDefault) builtinKnownHosts;
    };

    assertions =
      map (name: {
        assertion = validHost name;
        message = "neo.core.git: \"${name}\" is not a host name (expected e.g. github.com or git.example.com:2222).";
      })
      (lib.attrNames cfg.tokens ++ lib.attrNames cfg.knownHosts);

    users.groups.${group} = {};
    users.users = lib.genAttrs cfg.users (_: {extraGroups = [group];});

    environment.systemPackages = [gitSshKey];

    system.activationScripts.neo-git-ssh-key = {
      deps = ["users" "groups"];
      text = "${gitSshKey}/bin/neo-git-ssh-key ensure";
    };

    programs.ssh.knownHosts =
      lib.mapAttrs (name: key: {
        hostNames = [(splitHost name).known];
        publicKey = key;
      })
      cfg.knownHosts;

    # `Match all` closes the block so later ssh_config lines stay global.
    programs.ssh.extraConfig = lib.mkIf (sshHosts != [] && cfg.users != []) ''
      Match host ${lib.concatStringsSep "," sshHosts} localuser ${lib.concatStringsSep "," cfg.users}
        IdentityFile ${lib.neo.gitSshKey}
        IdentitiesOnly yes
      Match all
    '';

    programs.git = {
      enable = true;
      config = {
        user = {
          name = cfg.userName;
          email = cfg.userEmail;
        };
        credential.helper = lib.mkIf (cfg.tokens != {}) "${credentialHelper}";
      };
    };

    environment.etc =
      lib.mapAttrs' (host: token:
        lib.nameValuePair "neo/git/tokens/${host}" {
          text = token;
          mode = "0640";
          inherit group;
        })
      cfg.tokens
      // lib.optionalAttrs (cfg.tokens != {}) {
        "neo/git/access-tokens.conf" = {
          text = "access-tokens = ${lib.concatStringsSep " " (lib.mapAttrsToList (host: token: "${host}=${token}") cfg.tokens)}\n";
          mode = "0640";
          inherit group;
        };
      };

    nix.extraOptions = lib.mkIf (cfg.tokens != {}) ''
      !include ${accessTokensFile}
    '';
  };
}
