# openSystem Board Configuration: QEMU x86_64

## Target

QEMU virtual machine, x86_64 architecture.

## Purpose

Primary development and testing target. Used for:
- openSystem MVP validation
- CI/CD testing
- Demo recordings

## QEMU Command

```bash
qemu-system-x86_64 \
  -hda system.img \
  -m 8G \
  -smp 4 \
  -enable-kvm \
  -device virtio-gpu \
  -device virtio-keyboard-pci \
  -device virtio-mouse-pci \
  -device virtio-net-pci,netdev=net0 \
  -netdev user,id=net0 \
  -nographic
```

## Hardware Manifest

See `hardware_manifest_qemu.json` in the project root.

## Partition Layout

```
[EFI 512MB] [rootfs squashfs read-only] [data ext4 expandable]
```

## Buildroot Defconfig

This directory should contain `buildroot_defconfig` for board-specific
buildroot configuration. For development, the generic `qemu_x86_64_defconfig`
is used as fallback.
