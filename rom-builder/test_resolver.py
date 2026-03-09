#!/usr/bin/env python3
"""Quick test for hardware_resolver.py"""
import subprocess
import sys
import json
import tempfile
import os
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent

def test_resolver():
    manifest = {
        "board": "qemu_x86_64",
        "cpu": "qemu64",
        "ram_mb": 8192,
        "storage": [{"type": "virtio-blk", "size_mb": 16384}],
        "network": [{"type": "virtio-net"}],
        "display": "virtio-gpu",
        "input": ["virtio-keyboard", "virtio-mouse"]
    }

    with tempfile.NamedTemporaryFile(mode='w', suffix='.json', delete=False) as f:
        json.dump(manifest, f)
        manifest_path = f.name

    output_path = "/tmp/test_kconfig.fragment"

    try:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_DIR / "hardware_resolver.py"),
             "--manifest", manifest_path,
             "--output", output_path],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            print("FAIL: resolver returned non-zero")
            print(result.stderr)
            return False

        with open(output_path) as f:
            content = f.read()

        required = ["CONFIG_VIRTIO_BLK=y", "CONFIG_VIRTIO_NET=y",
                    "CONFIG_DRM_VIRTIO_GPU=y", "CONFIG_VIRTIO_INPUT=y",
                    "CONFIG_BPF_SYSCALL=y", "CONFIG_IO_URING=y"]

        for req in required:
            if req not in content:
                print(f"FAIL: missing {req} in kconfig output")
                return False

        print(f"PASS: resolver generated {len(content.splitlines())} kconfig lines")
        return True
    finally:
        os.unlink(manifest_path)
        if os.path.exists(output_path):
            os.unlink(output_path)

if __name__ == "__main__":
    ok = test_resolver()
    sys.exit(0 if ok else 1)
