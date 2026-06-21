#!/usr/bin/env python3
"""
Gobol Installer

Usage:
  python3 install.py                         # Install to default location
  python3 install.py --install-dir /path     # Install to custom location
  python3 install.py --no-build              # Skip build (use existing binaries)
  python3 install.py --uninstall             # Uninstall
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path
import argparse


__version__ = "0.1.0"

# ==================== Configuration ====================

def get_default_install_dir():
    if sys.platform == "win32":
        return Path(os.environ.get("USERPROFILE", Path.home())) / ".local" / "bin"
    else:
        return Path.home() / ".local" / "bin"


def get_binaries():
    binaries = ["gobol", "grape"]
    if sys.platform == "win32":
        return [b + ".exe" for b in binaries]
    return binaries


# ==================== Build ====================

def build_project():
    print("[INFO] Building Gobol...")
    result = subprocess.run(["cargo", "build", "--release"], capture_output=True, text=True)
    if result.returncode != 0:
        print("[FAIL] Build failed", file=sys.stderr)
        if result.stderr:
            print(result.stderr, file=sys.stderr)
        return False
    print("[ OK ] Build successful")
    return True


# ==================== Install Files ====================

def install_binaries(target_dir):
    binaries = get_binaries()
    target_dir.mkdir(parents=True, exist_ok=True)

    for name in binaries:
        src = Path("target/release") / name
        if not src.exists():
            print(f"[WARN] {name} not found, skipping")
            continue
        dst = target_dir / name
        shutil.copy2(src, dst)
        if sys.platform != "win32":
            dst.chmod(0o755)
        print(f"[ OK ] {name} -> {dst}")
    return True


def install_std(target_dir):
    src_std = Path("std")
    if not src_std.exists():
        return True
    dst_std = target_dir / "std"
    if dst_std.exists():
        shutil.rmtree(dst_std)
    shutil.copytree(src_std, dst_std)
    print(f"[ OK ] std/ -> {dst_std}")
    return True


# ==================== Add PATH (Cross-platform) ====================

def add_path_unix(target_dir):
    """Linux/macOS: add PATH to shell config file"""
    target_str = str(target_dir.absolute())
    shell = os.environ.get("SHELL", "")
    home = Path.home()

    if "zsh" in shell:
        config_file = home / ".zshrc"
    elif "bash" in shell:
        config_file = home / ".bashrc"
    elif "fish" in shell:
        config_file = home / ".config/fish/config.fish"
    else:
        config_file = home / ".profile"

    marker = "# Added by Gobol installer"
    lines = f"""

{marker}
export GOBOL_INSTALL_DIR="{target_str}"
export PATH="$GOBOL_INSTALL_DIR:$PATH"
"""

    if config_file.exists() and marker in config_file.read_text():
        print(f"[INFO] PATH already configured in {config_file}")
        return True

    try:
        config_file.parent.mkdir(parents=True, exist_ok=True)
        with open(config_file, "a") as f:
            f.write(lines)
        print(f"[ OK ] Added to {config_file}")
        print(f"[INFO] Run: source {config_file}  (or restart terminal)")
        return True
    except Exception as e:
        print(f"[WARN] Could not write to {config_file}: {e}")
        return False


def add_path_windows(target_dir):
    """Windows: add PATH using PowerShell (no 1024 char limit)"""
    target_str = str(target_dir.absolute())

    check_cmd = f'''
    $current = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($current -split ";" -contains "{target_str}") {{
        Write-Output "EXISTS"
    }} else {{
        Write-Output "NEW"
    }}
    '''
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", check_cmd],
        capture_output=True, text=True
    )

    if "EXISTS" in result.stdout:
        print(f"[INFO] {target_str} already in User PATH")
    else:
        add_cmd = f'''
        $newPath = "{target_str}"
        $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        if ($currentPath -notlike "*$newPath*") {{
            $updatedPath = if ($currentPath) {{ "$currentPath;$newPath" }} else {{ $newPath }}
            [Environment]::SetEnvironmentVariable("PATH", $updatedPath, "User")
            Write-Output "ADDED"
        }}
        '''
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command", add_cmd],
            capture_output=True, text=True
        )
        if "ADDED" in result.stdout:
            print(f"[ OK ] Added {target_str} to User PATH")
        else:
            print(f"[WARN] Could not add to PATH: {result.stderr}")

    set_var_cmd = f'''
    [Environment]::SetEnvironmentVariable("GOBOL_INSTALL_DIR", "{target_str}", "User")
    '''
    subprocess.run(["powershell", "-NoProfile", "-Command", set_var_cmd], capture_output=True)
    print(f"[ OK ] Set GOBOL_INSTALL_DIR={target_str}")

    print("")
    print("=" * 70)
    print("[INFO] Please restart your terminal for changes to take effect.")
    print("=" * 70)
    return True


def add_path(target_dir):
    if sys.platform == "win32":
        return add_path_windows(target_dir)
    else:
        return add_path_unix(target_dir)


# ==================== Uninstall ====================

def uninstall(target_dir):
    if not target_dir.exists():
        print(f"[FAIL] No installation found at {target_dir}")
        return False

    shutil.rmtree(target_dir)
    print(f"[ OK ] Uninstalled from {target_dir}")
    print("[INFO] PATH entries may still exist; remove them manually if needed")
    return True


# ==================== Main ====================

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Gobol Installer")
    parser.add_argument("--install-dir", help="Installation directory")
    parser.add_argument("--no-build", action="store_true", help="Skip building (use existing binaries)")
    parser.add_argument("--uninstall", action="store_true", help="Uninstall")
    parser.add_argument("--version", action="store_true", help="Show version and exit")
    args = parser.parse_args()

    if args.version:
        print(f"Gobol Installer {__version__}, Binding with Gobol {__version__}")
        sys.exit(0)

    if args.uninstall:
        target_dir = Path(args.install_dir) if args.install_dir else get_default_install_dir()
        target_dir /= "gobol"
        uninstall(target_dir)
        sys.exit(0)

    print("=" * 60)
    print("Gobol Installer")
    print("=" * 60)

    install_dir = args.install_dir or os.environ.get("GOBOL_INSTALL_DIR") or get_default_install_dir()

    if args.install_dir:
        # 命令行指定：加 /gobol
        target_dir = Path(install_dir) / "gobol"
    elif os.environ.get("GOBOL_INSTALL_DIR"):
        # 环境变量：直接使用（用户已经指定完整路径）
        target_dir = Path(install_dir)
    else:
        # 默认：加 /gobol
        target_dir = Path(install_dir) / "gobol"
    print(f"[INFO] Install directory: {target_dir}")

    if not args.no_build:
        if not build_project():
            sys.exit(1)

    print("")
    print("[INFO] Installing binaries...")
    install_binaries(target_dir)
    install_std(target_dir)

    print("")
    print("[INFO] Adding to PATH...")
    add_path(target_dir)

    print("")
    print("=" * 60)
    print("[ OK ] Installation complete!")
    print(f"      Binaries installed to: {target_dir}")
    print("=" * 60)

    print("")
    print("[INFO] Test your installation:")
    print("      gobol --version")
    print("      grape --help")
