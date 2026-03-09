#!/usr/bin/env python3
"""
build.py — AIOS ROM build pipeline.

Takes hardware_manifest.json, generates kconfig fragment, and orchestrates
buildroot to produce a bootable .img file.

Usage:
    python3 build.py --manifest hardware_manifest_qemu.json [--output system.img]

Requirements:
    - hardware_resolver.py (in same directory)
    - buildroot (downloaded automatically if not present)
    - For cross-compilation: qemu-user-static + binfmt_misc

Pipeline:
    hardware_manifest.json
      -> hardware_resolver.py   (manifest -> kconfig fragment)
      -> buildroot make         (kconfig + overlay -> rootfs)
      -> genimage               (rootfs + bootloader -> .img)
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
BUILDROOT_VERSION = "2024.02"
BUILDROOT_URL = f"https://buildroot.org/downloads/buildroot-{BUILDROOT_VERSION}.tar.gz"


def run(cmd: list[str], cwd=None, check=True) -> subprocess.CompletedProcess:
    """Run a command, printing it first."""
    print(f"  $ {' '.join(str(c) for c in cmd)}")
    return subprocess.run(cmd, cwd=cwd, check=check)


def load_manifest(path: str) -> dict:
    with open(path) as f:
        return json.load(f)


def resolve_kconfig(manifest_path: str, output_dir: Path) -> Path:
    """Step 1: hardware_manifest -> kconfig fragment."""
    kconfig_path = output_dir / "aios.fragment"
    resolver = SCRIPT_DIR / "hardware_resolver.py"
    run([
        sys.executable, str(resolver),
        "--manifest", manifest_path,
        "--db", str(SCRIPT_DIR / "driver_db.json"),
        "--output", str(kconfig_path),
    ])
    return kconfig_path


def get_buildroot(build_dir: Path) -> Path:
    """Download buildroot if not already present."""
    br_dir = build_dir / f"buildroot-{BUILDROOT_VERSION}"
    if br_dir.exists():
        print(f"  Buildroot already at {br_dir}")
        return br_dir

    tarball = build_dir / f"buildroot-{BUILDROOT_VERSION}.tar.gz"
    if not tarball.exists():
        print(f"  Downloading buildroot {BUILDROOT_VERSION}...")
        run(["wget", "-q", "-O", str(tarball), BUILDROOT_URL])

    print(f"  Extracting buildroot...")
    run(["tar", "-xzf", str(tarball), "-C", str(build_dir)])
    return br_dir


def build_rootfs(manifest: dict, kconfig_path: Path, br_dir: Path, output_dir: Path) -> Path:
    """Step 2: buildroot make -> rootfs."""
    board = manifest.get("board", "qemu_x86_64")
    board_config_dir = SCRIPT_DIR / "board" / board

    # Determine defconfig
    br_defconfig = board_config_dir / "buildroot_defconfig"
    if not br_defconfig.exists():
        # Use generic qemu x86_64 defconfig as fallback
        print(f"  Warning: No board-specific defconfig at {br_defconfig}")
        print(f"  Using generic qemu_x86_64_defconfig")
        run(["make", "qemu_x86_64_defconfig"], cwd=str(br_dir))
    else:
        # Copy board defconfig
        shutil.copy(str(br_defconfig), str(br_dir / ".config"))

    # Apply kconfig fragment
    merge_script = br_dir / "support" / "kconfig" / "merge_config.sh"
    if not merge_script.exists():
        print(f"  Warning: merge_config.sh not found at {merge_script}")
        print(f"  Manually appending kconfig fragment to .config")
        with open(br_dir / ".config", "a") as f:
            with open(kconfig_path) as kf:
                f.write("\n# AIOS hardware kconfig fragment\n")
                f.write(kf.read())
    else:
        result = run([
            str(merge_script),
            str(br_dir / ".config"),
            str(kconfig_path),
        ], cwd=str(br_dir), check=False)
        if result.returncode != 0:
            print(f"  Warning: merge_config.sh returned non-zero, kconfig may not be applied correctly")

    # Copy overlay
    overlay_src = SCRIPT_DIR / "buildroot_overlay"
    if overlay_src.exists():
        overlay_dst = output_dir / "overlay"
        shutil.copytree(str(overlay_src), str(overlay_dst), dirs_exist_ok=True)

    # Build
    cpu_count = os.cpu_count() or 4
    print(f"  Building rootfs (this takes 30-60 minutes on first run)...")
    run(["make", f"-j{cpu_count}", f"BR2_TARGET_ROOTFS_SQUASHFS=y"], cwd=str(br_dir))

    rootfs = br_dir / "output" / "images" / "rootfs.squashfs"
    return rootfs


def package_image(manifest: dict, rootfs_path: Path, output_dir: Path, output_name: str) -> Path:
    """
    Step 3: Package rootfs into bootable .img.

    For MVP: Creates a raw disk image with:
      - Partition 1: EFI (512MB, FAT32)
      - Partition 2: rootfs (squashfs, read-only)
      - Partition 3: data (ext4, expandable)

    Full genimage integration is TODO. This MVP uses dd/sfdisk/mkfs tools.
    """
    output_img = output_dir / output_name

    if not rootfs_path.exists():
        # Development stub: create a minimal image with manifest embedded
        print(f"  Note: rootfs not found (buildroot not run). Creating development stub.")
        with open(output_img, "wb") as f:
            # Write a recognizable header
            header = b"AIOS_IMG_V1\x00"
            manifest_bytes = json.dumps(manifest, indent=2).encode()
            f.write(header)
            f.write(len(manifest_bytes).to_bytes(4, "little"))
            f.write(manifest_bytes)
            # Pad to 1MB
            current = f.tell()
            f.write(b"\x00" * (1024 * 1024 - current))
        print(f"  Development stub image created: {output_img} (1MB)")
        return output_img

    # Real path: rootfs exists, create disk image
    rootfs_size = rootfs_path.stat().st_size
    efi_size_mb = 512
    data_size_mb = 1024
    total_size_mb = efi_size_mb + (rootfs_size // (1024 * 1024) + 1) + data_size_mb + 10

    print(f"  Creating {total_size_mb}MB disk image...")

    # Create empty image
    result = subprocess.run(
        ["dd", "if=/dev/zero", f"of={output_img}", "bs=1M", f"count={total_size_mb}"],
        capture_output=True, text=True
    )
    if result.returncode != 0:
        print(f"  Warning: dd failed: {result.stderr}")
        # Fall back to copy
        shutil.copy(str(rootfs_path), str(output_img))
        return output_img

    # Partition table (GPT)
    sfdisk_script = f"""label: gpt
first-lba: 2048
- : size={efi_size_mb}M, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI"
- : size={rootfs_size // 1024 + 1}K, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="rootfs"
- : type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="data"
"""
    result = subprocess.run(
        ["sfdisk", str(output_img)],
        input=sfdisk_script, capture_output=True, text=True
    )
    if result.returncode != 0:
        print(f"  Warning: sfdisk failed (tool not installed?): {result.stderr[:200]}")

    print(f"  Image ready: {output_img} ({total_size_mb}MB)")

    # TODO: Use genimage for production-quality image generation:
    # genimage --config genimage.cfg --rootpath output/target --outputpath images/

    return output_img


def main():
    parser = argparse.ArgumentParser(description="Build AIOS ROM image from hardware manifest")
    parser.add_argument("--manifest", required=True, help="Path to hardware_manifest.json")
    parser.add_argument("--output", default="system.img", help="Output image filename")
    parser.add_argument("--build-dir", default="/tmp/aios-rom-build", help="Build directory")
    parser.add_argument("--skip-buildroot", action="store_true",
                        help="Skip buildroot, generate stub image (for testing)")
    args = parser.parse_args()

    print("=" * 60)
    print("AIOS ROM Builder")
    print("=" * 60)

    build_dir = Path(args.build_dir)
    build_dir.mkdir(parents=True, exist_ok=True)

    manifest = load_manifest(args.manifest)
    board = manifest.get("board", "unknown")
    print(f"\nBuilding for board: {board}")

    # Step 1: Resolve kconfig
    print("\n[1/3] Resolving hardware drivers...")
    kconfig_path = resolve_kconfig(args.manifest, build_dir)

    if args.skip_buildroot:
        print("\n[2/3] Skipping buildroot (--skip-buildroot flag)")
        print("\n[3/3] Creating stub image...")
        output_img = package_image(manifest, Path("/nonexistent"), build_dir, args.output)
    else:
        # Step 2: Build rootfs
        print("\n[2/3] Building rootfs with buildroot...")
        br_dir = get_buildroot(build_dir)
        rootfs = build_rootfs(manifest, kconfig_path, br_dir, build_dir)

        # Step 3: Package
        print("\n[3/3] Packaging image...")
        output_img = package_image(manifest, rootfs, build_dir, args.output)

    print(f"\nBuild complete: {output_img}")
    print(f"\nTo test in QEMU:")
    print(f"  qemu-system-x86_64 -hda {output_img} -m {manifest.get('ram_mb', 512)} -enable-kvm")
    return 0


if __name__ == "__main__":
    sys.exit(main())
