#!/usr/bin/env python3
"""
Gobol Installer & Version Manager (rustup-style)

Layout created under ~/.gobol/:
    ~/.gobol/
        bin/                       # symlinks/copies of the active toolchain
            gobol[.exe]
            grape[.exe]
            gobolup[.exe]          # version-manager shim
        lib/                       # runtime files
            std/                   # Gobol standard library (*.gbl)
            c/                     # C companion runtime (*.c)
        versions/                  # one subdir per installed version
            v0.1.0/
                bin/
                lib/
            v0.2.0/
                bin/
                lib/
        active                     # text file: name of the active version (e.g. v0.1.0)
        env.{sh,fish,ps1}          # generated env files for sourcing

Usage:
    python3 install.py                          # build + install current dir as the active version
    python3 install.py --version-tag v0.2.0     # tag the install with a custom version
    python3 install.py --no-build               # skip `cargo build`
    python3 install.py --uninstall              # remove ~/.gobol entirely
    python3 install.py --list                   # list installed versions
    python3 install.py --switch v0.1.0          # switch active version
    python3 install.py --version                # show installer version
"""

import argparse
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

__version__ = "0.2.0"

# ==================== Platform detection ====================

def detect_platform():
    """Return (os_name, arch) where os_name ∈ {linux, macos, windows} and
    arch ∈ {x86_64, aarch64}."""
    raw_os = platform.system().lower()
    if raw_os == "linux":
        os_name = "linux"
    elif raw_os == "darwin":
        os_name = "macos"
    elif raw_os == "windows":
        os_name = "windows"
    else:
        os_name = raw_os

    raw_arch = platform.machine().lower()
    if raw_arch in ("x86_64", "amd64"):
        arch = "x86_64"
    elif raw_arch in ("aarch64", "arm64"):
        arch = "aarch64"
    else:
        arch = raw_arch

    return os_name, arch


def binary_suffix():
    return ".exe" if sys.platform == "win32" else ""


def is_windows():
    return sys.platform == "win32"


# ==================== Paths ====================

def gobol_home():
    """~/.gobol — the root of the install layout."""
    if override := os.environ.get("GOBOL_HOME"):
        return Path(override)
    return Path.home() / ".gobol"


def active_version_file():
    return gobol_home() / "active"


def versions_dir():
    return gobol_home() / "versions"


def bin_dir():
    return gobol_home() / "bin"


def lib_dir():
    return gobol_home() / "lib"


def read_active_version():
    f = active_version_file()
    if not f.exists():
        return None
    return f.read_text().strip() or None


def write_active_version(tag):
    active_version_file().write_text(tag + "\n")


def version_path(tag):
    return versions_dir() / tag


# ==================== Build ====================

def build_project(verbose=False):
    print("[INFO] Building Gobol (cargo build --release)...")
    cmd = ["cargo", "build", "--release", "--bins"]
    if verbose:
        result = subprocess.run(cmd)
    else:
        result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        print("[FAIL] Build failed", file=sys.stderr)
        if not verbose and result.stderr:
            print(result.stderr, file=sys.stderr)
        return False
    print("[ OK ] Build successful")
    return True


# ==================== Install one version ====================

def install_version(tag, no_build=False, verbose=False):
    """Build + install the current source tree as `tag` under ~/.gobol/versions/,
    then activate it (symlink/copy into ~/.gobol/bin)."""
    os_name, arch = detect_platform()
    print(f"[INFO] Platform: {os_name}/{arch}")

    # 0. Determine version tag
    if not tag:
        tag = read_cargo_version() or "v0.0.0-dev"
        # Ensure leading 'v'
        if not tag.startswith("v"):
            tag = "v" + tag
    print(f"[INFO] Installing version: {tag}")

    # 1. Build
    if not no_build:
        if not build_project(verbose=verbose):
            sys.exit(1)
    else:
        print("[INFO] Skipping build (--no-build)")

    # 2. Layout
    home = gobol_home()
    home.mkdir(parents=True, exist_ok=True)
    versions_dir().mkdir(parents=True, exist_ok=True)
    bin_dir().mkdir(parents=True, exist_ok=True)
    lib_dir().mkdir(parents=True, exist_ok=True)

    # 3. Versioned install: copy binaries + lib into versions/<tag>/
    vpath = version_path(tag)
    if vpath.exists():
        print(f"[INFO] Replacing existing version at {vpath}")
        shutil.rmtree(vpath)
    vbin = vpath / "bin"
    vlib = vpath / "lib"
    vbin.mkdir(parents=True)
    vlib.mkdir(parents=True)

    suffix = binary_suffix()
    binaries = [f"gobol{suffix}", f"grape{suffix}"]
    for name in binaries:
        src = Path("target/release") / name
        if not src.exists():
            print(f"[WARN] {name} not found in target/release, skipping")
            continue
        dst = vbin / name
        shutil.copy2(src, dst)
        if not is_windows():
            dst.chmod(0o755)
        print(f"[ OK ] {name} -> {dst}")

    # 4. Install stdlib + runtime C companions
    install_std_into(vlib)
    install_gobolup_into(vbin)

    # 5. Activate this version
    activate_version(tag)

    # 6. PATH setup
    configure_path()

    print()
    print("=" * 60)
    print(f"[ OK ] Gobol {tag} installed and activated")
    print(f"      Versions dir: {versions_dir()}")
    print(f"      Active bin:    {bin_dir()}")
    print(f"      Stdlib:        {lib_dir() / 'std'}")
    print("=" * 60)
    print()
    print_post_install_hint()


def install_std_into(target_lib):
    """Copy ./std and ./std/c into <target_lib>/std and <target_lib>/c."""
    src_std = Path("std")
    if not src_std.exists():
        print("[WARN] ./std not found; skipping stdlib install")
        return
    dst_std = target_lib / "std"
    if dst_std.exists():
        shutil.rmtree(dst_std)
    shutil.copytree(src_std, dst_std)
    print(f"[ OK ] std/ -> {dst_std}")

    # C companions
    src_c = src_std / "c"
    if src_c.exists():
        dst_c = target_lib / "c"
        if dst_c.exists():
            shutil.rmtree(dst_c)
        shutil.copytree(src_c, dst_c)
        print(f"[ OK ] std/c/ -> {dst_c}")


def install_gobolup_into(target_bin):
    """Install the gobolup shim (this script) into <target_bin>/gobolup."""
    self_path = Path(__file__).resolve()
    dst = target_bin / (f"gobolup{binary_suffix()}")
    try:
        shutil.copy2(self_path, dst)
        if not is_windows():
            dst.chmod(0o755)
        print(f"[ OK ] gobolup -> {dst}")
    except Exception as e:
        print(f"[WARN] Could not install gobolup: {e}")


# ==================== Activate / switch ====================

def activate_version(tag):
    """Make `tag` the active version: refresh ~/.gobol/bin and ~/.gobol/lib
    symlinks (Unix) or copies (Windows) pointing at versions/<tag>/."""
    vpath = version_path(tag)
    if not vpath.exists():
        print(f"[FAIL] Version {tag} not installed at {vpath}", file=sys.stderr)
        sys.exit(1)

    write_active_version(tag)
    refresh_active_links(tag)
    print(f"[ OK ] Active version: {tag}")


def refresh_active_links(tag=None):
    """Rebuild ~/.gobol/bin and ~/.gobol/lib from the active version dir."""
    if tag is None:
        tag = read_active_version()
    if tag is None:
        return
    vpath = version_path(tag)
    if not vpath.exists():
        return

    # bin/
    bdir = bin_dir()
    bdir.mkdir(parents=True, exist_ok=True)
    vbin = vpath / "bin"
    if vbin.exists():
        for entry in vbin.iterdir():
            dst = bdir / entry.name
            replace_link_or_copy(entry, dst)

    # lib/ — refresh whole tree
    ldir = lib_dir()
    ldir.mkdir(parents=True, exist_ok=True)
    vlib = vpath / "lib"
    if vlib.exists():
        for sub in vlib.iterdir():
            dst = ldir / sub.name
            if dst.exists() or dst.is_symlink():
                if dst.is_symlink() or dst.is_file():
                    dst.unlink()
                else:
                    shutil.rmtree(dst)
            replace_link_or_copy(sub, dst)


def replace_link_or_copy(src, dst):
    """Symlink on Unix, copy on Windows (symlinks need admin)."""
    if dst.exists() or dst.is_symlink():
        if dst.is_symlink() or dst.is_file():
            dst.unlink()
        else:
            shutil.rmtree(dst)
    if is_windows():
        if src.is_dir():
            shutil.copytree(src, dst)
        else:
            shutil.copy2(src, dst)
    else:
        try:
            os.symlink(src.resolve(), dst)
        except OSError:
            # Fallback to copy if symlinks fail (e.g. no permissions)
            if src.is_dir():
                shutil.copytree(src, dst)
            else:
                shutil.copy2(src, dst)


# ==================== PATH configuration ====================

def configure_path():
    bdir = bin_dir()
    target_str = str(bdir.absolute())

    # Detect an explicit GOBOL_HOME override BEFORE we set env vars ourselves.
    # If overridden (e.g. test/sandbox install), don't pollute the user's real
    # shell config — only write env files into GOBOL_HOME.
    home_overridden = bool(os.environ.get("GOBOL_HOME"))

    # Set GOBOL_INSTALL_DIR and GOBOL_HOME for this process
    set_env_var("GOBOL_HOME", str(gobol_home().absolute()))
    set_env_var("GOBOL_INSTALL_DIR", str(gobol_home().absolute()))

    if home_overridden:
        print("[INFO] GOBOL_HOME is overridden; skipping shell-config edits")
        write_env_files(target_str)
        return

    if is_windows():
        add_path_windows(target_str)
    else:
        add_path_unix(target_str)

    # Generate env files for sourcing
    write_env_files(target_str)


def set_env_var(key, value):
    """Best-effort persist an env var. On Unix we can't truly persist for the
    current shell; we rely on shell config. On Windows we use setx."""
    os.environ[key] = value
    if is_windows():
        try:
            subprocess.run(["setx", key, value], capture_output=True)
        except Exception:
            pass


def add_path_unix(target_str):
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
    fish = "fish" in str(config_file)
    if fish:
        block = (
            f"\n{marker}\n"
            f'set -gx GOBOL_HOME "{gobol_home()}"\n'
            f'set -gx GOBOL_INSTALL_DIR "{gobol_home()}"\n'
            f'fish_add_path "{target_str}"\n'
        )
    else:
        block = (
            f"\n{marker}\n"
            f'export GOBOL_HOME="{gobol_home()}"\n'
            f'export GOBOL_INSTALL_DIR="{gobol_home()}"\n'
            f'export PATH="$GOBOL_INSTALL_DIR/bin:$PATH"\n'
        )

    if config_file.exists() and marker in config_file.read_text():
        print(f"[INFO] PATH already configured in {config_file}")
        return

    try:
        config_file.parent.mkdir(parents=True, exist_ok=True)
        with open(config_file, "a") as f:
            f.write(block)
        print(f"[ OK ] Added to {config_file}")
    except Exception as e:
        print(f"[WARN] Could not write to {config_file}: {e}")


def add_path_windows(target_str):
    """Use PowerShell to set the User PATH persistently."""
    check = (
        '$current = [Environment]::GetEnvironmentVariable("PATH", "User"); '
        f'if ($current -split ";" -contains "{target_str}") {{ Write-Output "EXISTS" }} '
        'else { Write-Output "NEW" }'
    )
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", check],
        capture_output=True, text=True,
    )
    if "EXISTS" in result.stdout:
        print(f"[INFO] {target_str} already in User PATH")
    else:
        add_cmd = (
            '$p = [Environment]::GetEnvironmentVariable("PATH", "User"); '
            f'if ($p -notlike "*{target_str}*") {{ '
            f'[Environment]::SetEnvironmentVariable("PATH", '
            f'if ($p) {{ "$p;{target_str}" }} else {{ "{target_str}" }}, "User"); '
            'Write-Output "ADDED" }}'
        )
        r = subprocess.run(
            ["powershell", "-NoProfile", "-Command", add_cmd],
            capture_output=True, text=True,
        )
        if "ADDED" in r.stdout:
            print(f"[ OK ] Added {target_str} to User PATH")
        else:
            print(f"[WARN] Could not add to PATH: {r.stderr}")


def write_env_files(target_str):
    """Write env.sh / env.fish / env.ps1 in ~/.gobol/ for manual sourcing."""
    home = gobol_home()

    sh = home / "env.sh"
    sh.write_text(
        f'export GOBOL_HOME="{gobol_home()}"\n'
        f'export GOBOL_INSTALL_DIR="{gobol_home()}"\n'
        f'export PATH="$GOBOL_INSTALL_DIR/bin:$PATH"\n'
    )

    fish = home / "env.fish"
    fish.write_text(
        f'set -gx GOBOL_HOME "{gobol_home()}"\n'
        f'set -gx GOBOL_INSTALL_DIR "{gobol_home()}"\n'
        f'fish_add_path "{target_str}"\n'
    )

    ps1 = home / "env.ps1"
    ps1.write_text(
        f'$env:GOBOL_HOME = "{gobol_home()}"\n'
        f'$env:GOBOL_INSTALL_DIR = "{gobol_home()}"\n'
        f'$env:PATH = "{target_str};" + $env:PATH\n'
    )
    print(f"[ OK ] Env files written to {home}/env.{{sh,fish,ps1}}")


def print_post_install_hint():
    home_overridden = bool(os.environ.get("GOBOL_HOME")) and (
        os.environ.get("GOBOL_HOME") != str((Path.home() / ".gobol").absolute())
    )
    if home_overridden:
        print(f"[INFO] GOBOL_HOME is overridden — to use this install in a shell:")
        print(f'  source {gobol_home()}/env.sh')
        print(f"  (or set PATH={bin_dir()} manually)")
        return
    if is_windows():
        print("[INFO] Please restart your terminal for PATH changes to take effect.")
        return
    shell = os.environ.get("SHELL", "")
    if "zsh" in shell:
        rc = "~/.zshrc"
    elif "bash" in shell:
        rc = "~/.bashrc"
    elif "fish" in shell:
        rc = "~/.config/fish/config.fish"
    else:
        rc = "~/.profile"
    print(f"[INFO] Run: source {rc}   (or open a new terminal)")
    print("[INFO] Then test:  gobol --version")


# ==================== List / switch / uninstall ====================

def list_versions():
    vdir = versions_dir()
    if not vdir.exists():
        print("[INFO] No versions installed.")
        return
    active = read_active_version()
    installed = sorted(p.name for p in vdir.iterdir() if p.is_dir())
    if not installed:
        print("[INFO] No versions installed.")
        return
    print("Installed Gobol versions:")
    for v in installed:
        marker = " (active)" if v == active else ""
        print(f"  {v}{marker}")


def switch_version(tag):
    if not version_path(tag).exists():
        print(f"[FAIL] Version {tag} not found. Installed versions:", file=sys.stderr)
        list_versions()
        sys.exit(1)
    activate_version(tag)
    print()
    print(f"[ OK ] Switched to {tag}")
    print("[INFO] Open a new terminal or run:")
    print(f'  source {gobol_home()}/env.sh')


def uninstall():
    home = gobol_home()
    if not home.exists():
        print(f"[FAIL] No installation found at {home}")
        sys.exit(1)
    shutil.rmtree(home)
    print(f"[ OK ] Removed {home}")
    print("[INFO] Remove the PATH/GOBOL_* entries from your shell config manually if needed.")


# ==================== Helpers ====================

def read_cargo_version():
    """Read the `version` field from Cargo.toml."""
    try:
        text = Path("Cargo.toml").read_text()
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("version") and "=" in line:
                # version = "0.1.0"
                _, _, rhs = line.partition("=")
                return rhs.strip().strip('"').strip("'")
    except Exception:
        pass
    return None


# ==================== CLI ====================

def main():
    p = argparse.ArgumentParser(
        prog="gobolup",
        description="Gobol installer & version manager (rustup-style).",
    )
    p.add_argument("--version", action="store_true", help="Show installer version and exit")
    p.add_argument("--no-build", action="store_true", help="Skip `cargo build`")
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--version-tag", help="Tag this install with a custom version (e.g. v0.2.0)")
    p.add_argument("--uninstall", action="store_true", help="Remove ~/.gobol entirely")
    p.add_argument("--list", action="store_true", help="List installed versions")
    p.add_argument("--switch", metavar="TAG", help="Switch the active version to TAG")
    args = p.parse_args()

    if args.version:
        print(f"Gobol installer {__version__}")
        os_name, arch = detect_platform()
        print(f"Platform: {os_name}/{arch}")
        print(f"GOBOL_HOME: {gobol_home()}")
        return

    if args.uninstall:
        uninstall()
        return

    if args.list:
        list_versions()
        return

    if args.switch:
        switch_version(args.switch)
        return

    install_version(args.version_tag, no_build=args.no_build, verbose=args.verbose)


if __name__ == "__main__":
    main()
