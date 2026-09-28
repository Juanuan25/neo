# VM-specific hardware and virtualisation configuration.
# Stored under devices (not flake.modules.nixos) so it is NOT auto-included.
# Only configurations that explicitly import this module (e.g. QEMU via
# configurations.nixos.homeserver or nixosModules.vm) get these settings.
{
  config,
  self,
  ...
}: {
  devices.vm = {
    config,
    lib,
    pkgs,
    ...
  }: let
    # Same virtual size as the ext4 dev disk. The ZFS pool is created at this
    # size the first time the disko base image is formatted; a later change
    # does not resize an existing nixos-disko overlay.
    diskSizeMB = 1024000;
  in {
    # Development SSH key for `just ssh` / tools/development_ed25519.
    # Merged with any keys from settings.toml (listOf concatenates definitions).
    # Not a default on neo.core.ssh.authorizedKeys so real deployments stay clean.
    neo.core.ssh.authorizedKeys = [
      (lib.fileContents (self + "/tools/development_ed25519.pub"))
    ];

    users.allowNoPasswordLogin = true;
    users.mutableUsers = false;
    # boot.loader.grub.device = "/dev/vda";
    fileSystems."/".device = lib.mkDefault "/dev/vda1";
    fileSystems."/".fsType = lib.mkDefault "ext4";
    virtualisation = {
      diskSize = diskSizeMB;
      docker.enable = true;
      oci-containers.backend = "docker";
    };

    # Disko's vmWithDisko runner (lib/interactive-vm.nix) is a test helper:
    # it formats a qcow2 overlay under mktemp and deletes it when QEMU exits,
    # so /, docker, and /var/neo are empty on every boot. The guest system
    # still comes from the host Nix store (direct kernel boot + 9p), same as
    # run-nixos-vm, so keeping that overlay is what makes `just launch` persist.
    # The ext4 image stays nixos.qcow2. This chain is nixos-disko/.
    # disko.testMode is set only inside the variant; the guard stops the
    # variant from extending itself.
    virtualisation.vmVariantWithDisko = lib.mkIf (config.neo.disko.enabled && !config.disko.testMode) ({
      config,
      lib,
      pkgs,
      ...
    }: {
      disko.devices.disk.main.imageSize = "${toString diskSizeMB}M";
      # The image builder formats the pool inside its own QEMU. 1G OOMs on a 1TB vdev.
      disko.memSize = 2048;
      system.build.vmWithDisko = lib.mkForce (
        pkgs.writers.writeDashBin "disko-vm" ''
          set -eu
          export PATH=${lib.makeBinPath [pkgs.coreutils pkgs.qemu]}

          # Absolute: run-*-vm cds away before exec, and its drive path is "$tmp"/<name>.qcow2.
          disk_dir=''${DISKO_VM_STATE_DIR:-$(pwd)/nixos-disko}
          mkdir -p "$disk_dir/base"
          tmp=$(cd "$disk_dir" && pwd)
          export tmp
          # Do not share the ext4 VM's nixos-efi-vars.fd.
          export NIX_EFI_VARS="$tmp/efi-vars.fd"

          images=${lib.escapeShellArg config.system.build.diskoImages}
          vm_bin=${lib.escapeShellArg (config.system.build.vm + "/bin")}
          found=0
          for img in "$images"/*.qcow2; do
            [ -f "$img" ] || continue
            found=1
            name=$(basename "$img")
            overlay="$tmp/$name"
            base="$tmp/base/$name"
            if [ -f "$overlay" ]; then
              if [ ! -f "$base" ]; then
                echo "disko-vm: $overlay exists but backing file $base is missing." >&2
                echo "disko-vm: refusing to format a new disk over an existing overlay." >&2
                exit 1
              fi
              echo "disko-vm: reusing $overlay"
              continue
            fi
            echo "disko-vm: creating $overlay (nixos.qcow2 is not used)"
            rm -f "$base.new" "$overlay.new"
            cp --reflink=auto --sparse=always "$img" "$base.new"
            mv "$base.new" "$base"
            # Backing path is relative to the overlay, so the directory can be moved.
            qemu-img create -f qcow2 -b "base/$name" -F qcow2 "$overlay.new"
            mv "$overlay.new" "$overlay"
          done
          if [ "$found" -eq 0 ]; then
            echo "disko-vm: no qcow2 images in $images" >&2
            exit 1
          fi
          exec "$vm_bin"/run-*-vm "$@"
        ''
      );
    });
  };
  flake.nixosModules.vm = config.devices.vm;
}
