#!/usr/bin/env python3
"""
Gobol Installer TUI — Terminal User Interface for the Gobol toolchain.
Version management disabled, single global install only.
"""

import os
import sys
import time
import shutil
import platform
import subprocess
from pathlib import Path

# ==================== Terminal Colors (pure Python, no dependencies) ====================

class Colors:
    HEADER = '\033[95m'
    OKBLUE = '\033[94m'
    OKCYAN = '\033[96m'
    OKGREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    ENDC = '\033[0m'
    BOLD = '\033[1m'
    UNDERLINE = '\033[4m'
    GREY = '\033[90m'

def clear_screen():
    os.system('cls' if os.name == 'nt' else 'clear')

def print_menu(title, options, footer=""):
    clear_screen()
    print(f"{Colors.HEADER}{Colors.BOLD}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}{title:^60}{Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print()
    for i, option in enumerate(options, 1):
        print(f"{Colors.OKBLUE}{i:>2}{Colors.ENDC}. {option}")
    print()
    if footer:
        print(f"{Colors.GREY}{footer}{Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.GREY}Select a number, or press 'q' to quit{Colors.ENDC}")

def print_status(message, status_type="info"):
    if status_type == "info":
        print(f"{Colors.OKCYAN}[INFO]{Colors.ENDC} {message}")
    elif status_type == "ok":
        print(f"{Colors.OKGREEN}[ OK ]{Colors.ENDC} {message}")
    elif status_type == "warn":
        print(f"{Colors.WARNING}[WARN]{Colors.ENDC} {message}")
    elif status_type == "fail":
        print(f"{Colors.FAIL}[FAIL]{Colors.ENDC} {message}")

# ==================== Core Functions ====================

def detect_platform():
    raw_os = platform.system().lower()
    if raw_os == "linux": os_name = "linux"
    elif raw_os == "darwin": os_name = "macos"
    elif raw_os == "windows": os_name = "windows"
    else: os_name = raw_os
    raw_arch = platform.machine().lower()
    if raw_arch in ("x86_64", "amd64"): arch = "x86_64"
    elif raw_arch in ("aarch64", "arm64"): arch = "aarch64"
    else: arch = raw_arch
    return os_name, arch

def is_windows():
    return sys.platform == "win32"

def gobol_home():
    if override := os.environ.get("GOBOL_HOME"):
        return Path(override)
    return Path.home() / ".gobol"

# ==================== TUI Task Functions ====================

def task_build_and_install(no_build=False):
    """Build and install the Gobol toolchain (single global install, no version manager)."""
    clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   Build & Install Gobol Toolchain   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    
    os_name, arch = detect_platform()
    print_status(f"Platform: {os_name}/{arch}", "info")
    
    if not no_build:
        print_status("Building (cargo build --release)...", "info")
        cmd = ["cargo", "build", "--release", "--bins"]
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print_status("Build failed!", "fail")
            print(result.stderr)
            input("Press Enter to return to main menu...")
            return
        print_status("Build successful!", "ok")
    else:
        print_status("Skipping build (--no-build)", "warn")
    
    home = gobol_home()
    home.mkdir(parents=True, exist_ok=True)
    (home / "bin").mkdir(parents=True, exist_ok=True)
    (home / "lib").mkdir(parents=True, exist_ok=True)
    
    suffix = ".exe" if is_windows() else ""
    binaries = [f"gobol{suffix}", f"grape{suffix}", f"gobol-lsp{suffix}"]
    for name in binaries:
        src = Path("target/release") / name
        if not src.exists():
            print_status(f"{name} not found, skipping", "warn")
            continue
        dst = home / "bin" / name
        # 直接覆盖全局bin，无版本隔离
        if dst.exists():
            print_status(f"Overwriting existing {name}", "warn")
            dst.unlink()
        shutil.copy2(src, dst)
        if not is_windows():
            dst.chmod(0o755)
        print_status(f"{name} -> {dst}", "ok")
    
    print_status("Installing standard library...", "info")
    src_std = Path("std")
    dst_std = home / "lib" / "std"
    if src_std.exists():
        if dst_std.exists():
            shutil.rmtree(dst_std)
        shutil.copytree(src_std, dst_std)
        print_status(f"std/ -> {dst_std}", "ok")
    else:
        print_status("std/ directory not found", "warn")
    
    # 移除版本active标记、versions目录相关逻辑
    if not is_windows():
        shell = os.environ.get("SHELL", "")
        rc = Path.home() / (".zshrc" if "zsh" in shell else ".bashrc")
        marker = "# Added by Gobol installer"
        path_export = f'\nexport PATH="{home}/bin:$PATH"\n'
        rc_text = rc.read_text() if rc.exists() else ""
        if marker not in rc_text:
            with open(rc, "a") as f:
                f.write(marker + path_export)
            print_status(f"PATH added to {rc}", "ok")
    
    print_status("Installation complete! Gobol is installed globally.", "ok")
    input("Press Enter to return to main menu...")

def task_uninstall():
    clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.FAIL}{Colors.BOLD}   Uninstall Gobol   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print("\nWarning: This will permanently delete ~/.gobol/")
    confirm = input(f"{Colors.FAIL}Confirm uninstall? (type 'yes' to confirm): {Colors.ENDC}")
    if confirm.lower() != "yes":
        print_status("Uninstall cancelled.", "info")
        input("Press Enter to return to main menu...")
        return
    
    home = gobol_home()
    if home.exists():
        shutil.rmtree(home)
        print_status("Removed ~/.gobol", "ok")
        print_status("Please manually clean up your shell PATH in .bashrc/.zshrc", "warn")
    else:
        print_status("No installation found.", "warn")
    input("Press Enter to return to main menu...")

def task_install_plugin():
    clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   Install VS Code Plugin   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print()

    all_vsix = list(Path(".").rglob("*.vsix"))
    if not all_vsix:
        print_status("No .vsix plugin files found in project directory.", "fail")
        input("Press Enter to return to main menu...")
        return

    gobol_vsix = [f for f in all_vsix if f.name.startswith("vscode-gobol")]
    target_list = gobol_vsix if gobol_vsix else all_vsix

    selected: Path
    if len(target_list) == 1:
        selected = target_list[0]
        print_status(f"Auto found plugin: {selected.resolve()}", "ok")
    else:
        # 多个vsix，列出让用户选择
        print_status(f"Found {len(target_list)} plugin packages, please select:", "info")
        for idx, file in enumerate(target_list, 1):
            print(f"  {idx}. {file.resolve()}")
        while True:
            raw = input(f"{Colors.OKBLUE}Input number: {Colors.ENDC}").strip()
            if raw.isdigit():
                num = int(raw)
                if 1 <= num <= len(target_list):
                    selected = target_list[num - 1]
                    break
            print_status("Invalid number, try again", "warn")

    vsix_path = selected

    try:
        subprocess.run(["code", "--version"], capture_output=True, check=True)
    except (subprocess.CalledProcessError, FileNotFoundError):
        print_status("VS Code 'code' command not found.", "warn")
        print_status(f"Manual install file path: {vsix_path.resolve()}", "info")
        input("Press Enter to return to main menu...")
        return

    print_status(f"Installing VS Code extension: {vsix_path.name}", "info")
    result = subprocess.run(
        ["code", "--install-extension", str(vsix_path)],
        capture_output=True, text=True
    )
    if result.returncode == 0:
        print_status("Plugin installed successfully!", "ok")
        print_status("Please restart VS Code to activate it.", "info")
    else:
        print_status("Plugin installation failed.", "fail")
        print(result.stderr)
    input("Press Enter to return to main menu...")

# ==================== Main TUI Loop ====================

def main():
    while True:
        # 删除版本列表选项，仅保留3个核心功能+退出
        options = [
            "Build & Install Gobol",
            "Install VS Code Plugin",
            "Uninstall Gobol",
            "Exit"
        ]
        print_menu(
            "Gobol Installer",
            options,
            footer=f"GOBOL_HOME: {gobol_home()}"
        )
        choice = input(f"{Colors.OKCYAN}❯ {Colors.ENDC}").strip().lower()

        if choice == "q":
            break
        elif choice == "1":
            task_build_and_install()
        elif choice == "2":
            task_install_plugin()
        elif choice == "3":
            task_uninstall()
        elif choice == "4":
            print(f"{Colors.OKCYAN}Goodbye!{Colors.ENDC}")
            break
        else:
            print_status("Invalid choice.", "warn")
            time.sleep(1)

if __name__ == "__main__":
    main()