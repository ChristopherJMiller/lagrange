{ config, lib, pkgs, modulesPath, inputs, ... }:

let
  # The flake ref disko-install + nixos-install consume. Override by passing
  # `--flake <ref>#lagrange` to `lagrange-install` if you're testing a fork.
  defaultFlake = "github:christopherjmiller/lagrange#lagrange";

  lagrange-install = pkgs.writeShellApplication {
    name = "lagrange-install";
    runtimeInputs = with pkgs; [
      coreutils
      util-linux
      inputs.disko.packages.${pkgs.system}.disko-install
    ];
    text = ''
      set -euo pipefail

      FLAKE="${defaultFlake}"
      DISK=""
      FORCE=0

      usage() {
        cat <<EOF
      Usage: lagrange-install [--disk /dev/<X>] [--flake <ref>#lagrange] [--force]

      Partitions the target disk with disko, installs the lagrange NixOS
      configuration, then pauses so you can drop the sops age key before
      rebooting.

      Refuses to run against a disk that already carries a lagrange
      installation (disk-main-root / disk-main-ESP partlabels present)
      unless --force is supplied. This is intentional — re-running this
      command wipes the disk, including the sops age key.
      EOF
      }

      while [[ $# -gt 0 ]]; do
        case "$1" in
          --disk)  DISK="$2"; shift 2 ;;
          --flake) FLAKE="$2"; shift 2 ;;
          --force) FORCE=1; shift ;;
          -h|--help) usage; exit 0 ;;
          *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
        esac
      done

      echo "=== Lagrange installer ==="
      echo
      echo "Available disks:"
      # `MODEL` can contain spaces, so $4 is unreliable — match on the last
      # column (TYPE) with $NF instead. Use `-p` to print full /dev/<X> paths.
      lsblk -dp -o NAME,SIZE,MODEL,TYPE | awk 'NR==1 || $NF=="disk"'
      echo

      if [[ -z "$DISK" ]]; then
        read -rp "Target disk (e.g. /dev/nvme0n1): " DISK
      fi

      if [[ ! -b "$DISK" ]]; then
        echo "Not a block device: $DISK" >&2
        exit 1
      fi

      # Already-installed guard: disko stamps these partlabels on every
      # lagrange disk. If they're present, refuse without --force so an
      # accidental re-run doesn't wipe the sops key and rootfs.
      if [[ -b /dev/disk/by-partlabel/disk-main-root \
         || -b /dev/disk/by-partlabel/disk-main-ESP ]]; then
        if [[ "$FORCE" != "1" ]]; then
          echo
          echo "Refusing to reinstall — this disk already carries a lagrange" >&2
          echo "installation (disk-main-root or disk-main-ESP partlabel present)." >&2
          echo "If you really want to wipe and reinstall, pass --force." >&2
          echo
          echo "If you just need to re-apply config changes to an existing" >&2
          echo "install, boot the installed system and let comin reconcile," >&2
          echo "or mount the rootfs at /mnt and run nixos-install --root /mnt" >&2
          echo "--flake <ref> from this ISO (no reformat)." >&2
          exit 1
        fi
        echo
        echo "--force given: proceeding with destructive reinstall."
      fi

      echo
      echo "About to ERASE $DISK and install $FLAKE on it."
      read -rp "Type the disk path again to confirm: " CONFIRM
      if [[ "$CONFIRM" != "$DISK" ]]; then
        echo "Mismatch, aborting." >&2
        exit 1
      fi

      echo
      echo "Partitioning and installing — this will take several minutes..."
      disko-install --flake "$FLAKE" --disk main "$DISK"

      echo
      cat <<'EOF'

      === Installation complete ===

      Next step: drop your sops age private key onto the new rootfs so
      first boot can decrypt secrets/satellite.yaml.

      Easiest path from your workstation:

        scp ~/.config/sops/age/lagrange-host.txt root@<this-host>:/tmp/key.txt

      Then back on this installer:

        install -D -m 600 /tmp/key.txt /mnt/var/lib/sops-nix/key.txt

      Press Enter once the key is in place to reboot. Ctrl-C to bail.
      EOF

      read -r _

      if [[ ! -s /mnt/var/lib/sops-nix/key.txt ]]; then
        echo
        echo "Warning: /mnt/var/lib/sops-nix/key.txt is missing or empty."
        echo "Without it, sops-nix activation will fail on first boot."
        read -rp "Continue reboot anyway? [y/N] " ANS
        case "$ANS" in
          y|Y) ;;
          *) echo "Aborting reboot. Key was not staged."; exit 1 ;;
        esac
      fi

      echo "Rebooting in 5s..."
      sleep 5
      reboot
    '';
  };
in
{
  # Minimal installer ISO. Carries:
  #   - the operator's SSH public key (so they can finish provisioning remotely)
  #   - the lagrange-install wrapper (disko + nixos-install + key-drop pause)
  #   - wireguard-tools (the actual private key lands via sops after first boot)
  #
  # After `lagrange-install` reboots into the real system, comin owns the
  # box — no more SSH-driven imperative steps.

  imports = [
    "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"
  ];

  image.baseName = lib.mkForce "lagrange-installer";
  isoImage = {
    makeEfiBootable = true;
    makeUsbBootable = true;
  };

  networking.hostName = "lagrange-installer";

  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICHR4q3amhKDhCF6+xa3oTXJX2ycN503+cEo/gpnOkFt git@chrismiller.xyz"
  ];

  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "prohibit-password";
  };

  # Tools needed during install.
  environment.systemPackages = with pkgs; [
    git
    wireguard-tools
    sops
    age
    nixos-install-tools
    parted
    gptfdisk
    lagrange-install
  ];

  # Convenience banner so the operator knows what they're looking at.
  services.getty.helpLine = lib.mkForce ''
    Lagrange installer ISO. To provision this box, run:
        sudo lagrange-install
    It will partition the target disk, install NixOS, and pause for you to
    drop your sops age key before rebooting. comin takes over from there.
  '';
}
