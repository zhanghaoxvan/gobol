#!/usr/bin/env python3
"""Gobol toolchain installer with an interactive GRUB-like menu and CLI mode."""

import argparse
import os
import sys
import time
import shutil
import platform
import subprocess
from pathlib import Path

# ==================== Terminal Colors ====================

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
    subprocess.run('cls' if os.name == 'nt' else 'clear', shell=True)

def print_menu(title, options, selected=0, footer=""):
    clear_screen()
    print(f"{Colors.HEADER}{Colors.BOLD}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}{title:^60}{Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print()
    for i, option in enumerate(options, 1):
        prefix = ">" if i - 1 == selected else " "
        color = Colors.OKGREEN if i - 1 == selected else Colors.OKBLUE
        print(f"{color}{prefix} {i:>2}. {option}{Colors.ENDC}")
    print()
    if footer:
        print(f"{Colors.GREY}{footer}{Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.GREY}Use ↑/↓, Enter to select, or press 'q' to quit{Colors.ENDC}")


def choose_menu(title, options, footer=""):
    """Return a menu index using arrow keys, with a line-input fallback."""
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        return int(input("Select an option: ").strip()) - 1

    selected = 0
    if is_windows():
        import msvcrt
        while True:
            print_menu(title, options, selected, footer)
            key = msvcrt.getwch()
            if key in ("\r", "\n"):
                return selected
            if key.lower() == "q":
                return -1
            if key in ("0", "1", "2", "3", "4", "5", "6", "7", "8", "9"):
                choice = int(key) - 1
                if 0 <= choice < len(options):
                    return choice
            if key == "\x00" or key == "\xe0":
                key = msvcrt.getwch()
                if key == "H":
                    selected = (selected - 1) % len(options)
                elif key == "P":
                    selected = (selected + 1) % len(options)
    else:
        import termios
        import tty
        old_settings = termios.tcgetattr(sys.stdin)
        try:
            tty.setcbreak(sys.stdin.fileno())
            while True:
                print_menu(title, options, selected, footer)
                key = sys.stdin.read(1)
                if key in ("\r", "\n"):
                    return selected
                if key.lower() == "q":
                    return -1
                if key.isdigit():
                    choice = int(key) - 1
                    if 0 <= choice < len(options):
                        return choice
                if key == "\x1b":
                    sequence = sys.stdin.read(2)
                    if sequence == "[A":
                        selected = (selected - 1) % len(options)
                    elif sequence == "[B":
                        selected = (selected + 1) % len(options)
        finally:
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old_settings)

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
    if override := os.environ.get("GOBOL_INSTALL_DIR"):
        return Path(override)
    if override := os.environ.get("GOBOL_HOME"):
        return Path(override)
    return Path.home() / ".gobol"

# ==================== TUI Task Functions ====================

def task_build_and_install(no_build=False, install_dir=None, pause=True):
    """Build and install the Gobol toolchain with user-defined installation directory."""
    if pause:
        clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   Build & Install Gobol Toolchain   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    
    os_name, arch = detect_platform()
    print_status(f"Platform: {os_name}/{arch}", "info")
    
    # ----- 用户选择安装目录 -----
    default_install_dir = Path.home() / ".gobol"
    current_env = os.environ.get("GOBOL_INSTALL_DIR") or os.environ.get("GOBOL_HOME")
    if current_env:
        default_install_dir = Path(current_env)
    
    if install_dir is None:
        print(f"\n{Colors.OKCYAN}Current installation directory: {Colors.ENDC}{default_install_dir}")
        user_input = input(f"{Colors.OKCYAN}Enter new installation directory (or press Enter to keep current): {Colors.ENDC}").strip()
        install_dir = Path(user_input).expanduser().resolve() if user_input else default_install_dir
    else:
        install_dir = Path(install_dir).expanduser().resolve()
    
    print_status(f"Installation directory set to: {install_dir}", "ok")
    
    # ----- 构建 -----
    if not no_build:
        print_status("Building (cargo build --release)...", "info")
        cmd = ["cargo", "build", "--release", "--bins"]
        result = subprocess.run(cmd, capture_output=True, text=True, cwd=Path(__file__).resolve().parent)
        if result.returncode != 0:
            print_status("Build failed!", "fail")
            print(result.stderr)
            if pause:
                input("Press Enter to return to main menu...")
            return False
        print_status("Build successful!", "ok")
    else:
        print_status("Skipping build (--no-build)", "warn")
    
    # ----- 安装 -----
    install_dir.mkdir(parents=True, exist_ok=True)
    (install_dir / "bin").mkdir(parents=True, exist_ok=True)
    (install_dir / "lib").mkdir(parents=True, exist_ok=True)
    
    suffix = ".exe" if is_windows() else ""
    binaries = [f"gobol{suffix}", f"grape{suffix}", f"gobol-lsp{suffix}"]
    for name in binaries:
        src = Path(__file__).resolve().parent / "target/release" / name
        if not src.exists():
            print_status(f"{name} not found, skipping", "warn")
            continue
        dst = install_dir / "bin" / name
        if dst.exists():
            print_status(f"Overwriting existing {name}", "warn")
            dst.unlink()
        shutil.copy2(src, dst)
        if not is_windows():
            dst.chmod(0o755)
        print_status(f"{name} -> {dst}", "ok")
    
    print_status("Installing standard library...", "info")
    src_std = Path(__file__).resolve().parent / "std"
    dst_std = install_dir / "lib" / "std"
    if src_std.exists():
        if dst_std.exists():
            shutil.rmtree(dst_std)
        shutil.copytree(src_std, dst_std)
        print_status(f"std/ -> {dst_std}", "ok")
    else:
        print_status("std/ directory not found", "warn")
    
    # ----- 写入环境变量到shell配置文件 -----
    if not is_windows():
        shell = os.environ.get("SHELL", "")
        rc = Path.home() / (".zshrc" if "zsh" in shell else ".bashrc")
        marker = "# Added by Gobol installer"
        
        rc_text = rc.read_text() if rc.exists() else ""
        lines = rc_text.splitlines()
        new_lines = []
        skip = False
        for line in lines:
            if line.strip() == marker:
                skip = True
                continue
            if skip and line.strip().startswith("export GOBOL_HOME="):
                continue
            if skip and line.strip().startswith("export GOBOL_INSTALL_DIR="):
                continue
            if skip and line.strip().startswith("export PATH=") and "GOBOL_HOME" in line:
                continue
            if skip and line == "":
                continue
            if skip and line.strip() == "":
                continue
            if skip and line.strip() and not line.strip().startswith("#"):
                skip = False
                new_lines.append(line)
            elif not skip:
                new_lines.append(line)
        
        with open(rc, "w") as f:
            f.write("\n".join(new_lines))
            f.write(f"\n\n{marker}")
            f.write(f'\nexport GOBOL_HOME="{install_dir}"')
            f.write(f'\nexport GOBOL_INSTALL_DIR="{install_dir}"')
            f.write(f'\nexport PATH="$GOBOL_HOME/bin:$PATH"\n')
        
        print_status(f"Environment variables added to {rc}", "ok")
        print_status(f"Please run: source {rc}  OR restart your terminal", "info")
    else:
        print_status("Setting system environment variables...", "info")
        subprocess.run(f'setx GOBOL_HOME "{install_dir}"', shell=True)
        subprocess.run(f'setx GOBOL_INSTALL_DIR "{install_dir}"', shell=True)
        subprocess.run(f'setx PATH "%PATH%;{install_dir}\\bin"', shell=True)
        print_status("Environment variables set. Please restart your terminal.", "info")
    
    print_status("Installation complete! Gobol is installed globally.", "ok")
    if pause:
        input("Press Enter to return to main menu...")
    return True

def task_uninstall(install_dir=None, assume_yes=False, pause=True):
    if pause:
        clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.FAIL}{Colors.BOLD}   Uninstall Gobol   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print("\nWarning: This will permanently delete the Gobol installation directory.")
    install_dir = Path(install_dir).expanduser().resolve() if install_dir else gobol_home()
    print(f"Installation directory: {install_dir}")
    confirm = "yes" if assume_yes else input(f"{Colors.FAIL}Confirm uninstall? (type 'yes' to confirm): {Colors.ENDC}")
    if confirm.lower() != "yes":
        print_status("Uninstall cancelled.", "info")
        if pause:
            input("Press Enter to return to main menu...")
        return
    
    if install_dir.exists():
        shutil.rmtree(install_dir)
        print_status(f"Removed {install_dir}", "ok")
        print_status("Please manually clean up your shell PATH in .bashrc/.zshrc", "warn")
    else:
        print_status("No installation found.", "warn")
    if pause:
        input("Press Enter to return to main menu...")

def task_extension_guide(pause=True):
    """显示 VS Code 和 Neovim 扩展安装指南（跨平台命令）"""
    if pause:
        clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   VS Code & Neovim Extension Guide   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print()

    project_root = Path(__file__).resolve().parent
    vscode_ext_path = project_root / "vscode-gobol"
    nvim_ext_path = project_root / "nvim-gobol"
    is_windows = platform.system().lower() == "windows"

    # ===== 检测当前 shell =====
    # PowerShell 特有的环境变量
    is_pwsh = "PSModulePath" in os.environ

    # ========== VS Code ==========
    print(f"{Colors.BOLD}{Colors.OKGREEN}┌─ VS Code Extension{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│  Location: {vscode_ext_path}{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│  {Colors.BOLD}Build:{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    cd {vscode_ext_path}{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    npm install{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    npm run build{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    npm install -g @vscode/vsce{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    vsce package{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│  {Colors.BOLD}Install:{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    code --install-extension ./vscode-gobol-*.vsix{Colors.ENDC}")
    print(f"{Colors.OKGREEN}└─{Colors.ENDC}")

    # ========== Neovim ==========
    print()
    print(f"{Colors.BOLD}{Colors.OKBLUE}┌─ Neovim Extension{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│  Location: {nvim_ext_path}{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│{Colors.ENDC}")

    if is_windows:
        if is_pwsh:
            copy_cmd = f'Copy-Item -Recurse -Force "{nvim_ext_path}" "$env:USERPROFILE\\AppData\\Local\\nvim\\pack\\plugins\\start\\gobol"'
        else:
            copy_cmd = f'xcopy /E /I "{nvim_ext_path}" "%USERPROFILE%\\AppData\\Local\\nvim\\pack\\plugins\\start\\gobol"'
    else:
        if is_pwsh:
            copy_cmd = f'Copy-Item -Recurse -Force "{nvim_ext_path}" "$HOME/.config/nvim/pack/plugins/start/gobol"'
        else:
            copy_cmd = f'cp -r {nvim_ext_path} ~/.config/nvim/pack/plugins/start/gobol'

    print(f"{Colors.OKBLUE}│  {Colors.BOLD}Install (manual):{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│    {copy_cmd}{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│  {Colors.BOLD}Or with lazy.nvim:{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│    {{{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│      dir = \"~/gobol/nvim-gobol\",{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│      ft = \"gobol\",{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│      config = function(){Colors.ENDC}")
    print(f"{Colors.OKBLUE}│        vim.cmd(\"packadd gobol\"){Colors.ENDC}")
    print(f"{Colors.OKBLUE}│      end,{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│    }}{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│  {Colors.BOLD}Note:{Colors.ENDC}")
    print(f"{Colors.OKBLUE}│    Ensure Gobol LSP is in PATH: ~/.gobol/bin{Colors.ENDC}")
    print(f"{Colors.OKBLUE}└─{Colors.ENDC}")

    if pause:
        input(f"{Colors.GREY}Press Enter to return to main menu...{Colors.ENDC}")

# ==================== CLI and Main TUI Loop ====================

def parse_args():
    parser = argparse.ArgumentParser(
        description="Build and install the Gobol toolchain."
    )
    parser.add_argument(
        "command",
        nargs="?",
        choices=("install", "uninstall", "extensions"),
        help="run a task without opening the interactive menu",
    )
    parser.add_argument(
        "--install-dir",
        metavar="PATH",
        help="installation directory (also honored by GOBOL_INSTALL_DIR)",
    )
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="install existing release binaries without running Cargo",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="confirm destructive actions without prompting",
    )
    parser.add_argument(
        "--non-interactive",
        "--ci",
        dest="non_interactive",
        action="store_true",
        help="never open the TUI or prompt; defaults to install",
    )
    return parser.parse_args()


def run_cli(args):
    command = args.command or "install"
    if command == "install":
        ok = task_build_and_install(
            no_build=args.no_build,
            install_dir=args.install_dir,
            pause=False,
        )
        return 0 if ok else 1
    if command == "uninstall":
        if not args.yes:
            print_status("uninstall requires --yes in non-interactive mode.", "fail")
            return 2
        task_uninstall(
            install_dir=args.install_dir,
            assume_yes=True,
            pause=False,
        )
        return 0
    task_extension_guide(pause=False)
    return 0


def main():
    args = parse_args()
    if (
        args.command
        or args.non_interactive
        or args.install_dir
        or args.no_build
        or args.yes
    ):
        return run_cli(args)

    while True:
        options = [
            "Build & Install Gobol",
            "Extension Guide (VS Code & Neovim)",
            "Uninstall Gobol",
            "Exit"
        ]
        choice = choose_menu(
            "Gobol Installer",
            options,
            footer=f"GOBOL_INSTALL_DIR: {gobol_home()}"
        )

        if choice == -1:
            break
        elif choice == 0:
            task_build_and_install()
        elif choice == 1:
            task_extension_guide()
        elif choice == 2:
            task_uninstall()
        elif choice == 3:
            print(f"{Colors.OKCYAN}Goodbye!{Colors.ENDC}")
            break
        else:
            print_status("Invalid choice.", "warn")
            time.sleep(1)

if __name__ == "__main__":
    sys.exit(main())
