#!/usr/bin/env python3
"""Gobol toolchain installer with an interactive GRUB-like menu and CLI mode."""

import argparse
import os
import queue
import sys
import threading
import time
import shutil
import platform
import subprocess
from pathlib import Path

# Windows registry imports (only load if on Windows)
if sys.platform == "win32":
    import winreg
    import ctypes

MARKER = "# Added by Gobol installer"

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
            if key in ("\x00", "\xe0"):
                key = msvcrt.getwch()
                if key == "H":
                    selected = (selected - 1) % len(options)
                elif key == "P":
                    selected = (selected + 1) % len(options)
    else:
        import termios
        import tty
        old_settings = termios.tcgetattr(sys.stdin) # type: ignore
        try:
            tty.setcbreak(sys.stdin.fileno()) # type: ignore
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
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old_settings) # type: ignore

def print_status(message, status_type="info"):
    if status_type == "info":
        print(f"{Colors.OKCYAN}[INFO]{Colors.ENDC} {message}")
    elif status_type == "ok":
        print(f"{Colors.OKGREEN}[ OK ]{Colors.ENDC} {message}")
    elif status_type == "warn":
        print(f"{Colors.WARNING}[WARN]{Colors.ENDC} {message}")
    elif status_type == "fail":
        print(f"{Colors.FAIL}[FAIL]{Colors.ENDC} {message}")
    elif status_type == "error":
        print(f"{Colors.FAIL}[ERR ]{Colors.ENDC} {message}")

def run_build_with_progress(cmd, cwd):
    """Stream Cargo's build status through a terminal-friendly progress bar."""
    try:
        process = subprocess.Popen(
            cmd,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
    except OSError as error:
        print_status(f"Could not start Cargo build: {error}", "fail")
        return 127
    if process.stdout is None:
        raise RuntimeError("Cargo build output pipe was not created")

    lines = queue.Queue()

    def read_output():
        for line in process.stdout: # type: ignore
            lines.put(line)
        lines.put(None)

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()

    interactive = sys.stdout.isatty()
    output = []
    latest = "Waiting for compiler output..."
    frame = 0
    finished = False
    while not finished:
        try:
            line = lines.get(timeout=0.1)
        except queue.Empty:
            line = ""

        if line is None:
            finished = True
        elif line:
            output.append(line)
            latest = line.strip() or latest
            if not interactive:
                sys.stdout.write(line)
                sys.stdout.flush()

        if interactive and not finished:
            width = 20
            start = frame % (width + 5) - 5
            bar = [" "] * width
            for offset in range(5):
                position = start + offset
                if 0 <= position < width:
                    bar[position] = "="
            bar[min(max(start + 5, 0), width - 1)] = ">"
            status = f"Building [{''.join(bar)}] {latest}"
            sys.stdout.write(f"\r\033[2K{status[:120]}")
            sys.stdout.flush()
            frame += 1

    return_code = process.wait()
    reader.join()
    if interactive:
        sys.stdout.write("\r\033[2K")
        if return_code != 0:
            sys.stdout.writelines(output)
        sys.stdout.flush()
    return return_code

# ==================== Core Functions ====================

def detect_platform():
    os_raw = platform.system().lower()
    arch_raw = platform.machine().lower()
    os_name = {"windows":"windows", "linux":"linux", "darwin":"macos"}.get(os_raw, os_raw)
    arch = {"x86_64":"x86_64", "amd64":"x86_64", "aarch64":"aarch64", "arm64":"aarch64"}.get(arch_raw, arch_raw)
    return os_name, arch

def is_windows():
    return sys.platform == "win32"

def gobol_home():
    if override := os.environ.get("GOBOL_INSTALL_DIR"):
        return Path(override)
    if override := os.environ.get("GOBOL_HOME"):
        return Path(override)
    return Path.home() / ".gobol"

def broadcast_env_change():
    if is_windows():
        HWND_BROADCAST = 0xFFFF
        WM_SETTINGCHANGE = 0x001A
        SMTO_ABORTIFHUNG = 0x0002
        ctypes.windll.user32.SendMessageTimeoutW(
            HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment", SMTO_ABORTIFHUNG, 5000, None
        )

# ==================== TUI Task Functions ====================

def task_build_and_install(no_build=False, install_dir=None, pause=True):
    if pause:
        clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   Build & Install Gobol Toolchain   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    
    os_name, arch = detect_platform()
    print_status(f"Platform: {os_name}/{arch}", "info")
    
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
    
    if not no_build:
        print_status("Compiling Gobol toolchain", "info")
        cmd = ["cargo", "build", "--release", "--bins", "--message-format=human"]
        return_code = run_build_with_progress(
            cmd,
            cwd=Path(__file__).resolve().parent,
        )
        if return_code != 0:
            print_status("Build failed!", "fail")
            if pause:
                input("Press Enter to return to main menu...")
            return False
        print_status("Build successful!", "ok")
    else:
        print_status("Skipping build (--no-build)", "warn")
    
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
    
    # ----- 写入环境变量 -----
    if not is_windows():
        shell = os.environ.get("SHELL", "")
        rc = Path.home() / (".zshrc" if "zsh" in shell else ".bashrc")
        
        rc_text = rc.read_text() if rc.exists() else ""
        lines = rc_text.splitlines()
        new_lines = []
        
        # 精准过滤
        for line in lines:
            stripped = line.strip()
            if stripped == MARKER: continue
            if stripped.startswith("export GOBOL_HOME="): continue
            if stripped.startswith("export GOBOL_INSTALL_DIR="): continue
            if stripped.startswith("export PATH=") and "GOBOL_HOME" in line: continue
            new_lines.append(line)
        
        # 追加新块
        with open(rc, "w") as f:
            f.write("\n".join(new_lines))
            f.write(f"\n\n{MARKER}")
            f.write(f'\nexport GOBOL_HOME="{install_dir}"')
            f.write(f'\nexport GOBOL_INSTALL_DIR="{install_dir}"')
            f.write(f'\nexport PATH="$GOBOL_HOME/bin:$PATH"\n')
        
        print_status(f"Environment variables added to {rc}", "ok")
        print_status(f"Please run: source {rc}  OR restart your terminal", "info")
    else:
        print_status("Setting system environment variables via Registry...", "info")
        bin_path = str(install_dir / "bin")
        
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_ALL_ACCESS) as key:
                winreg.SetValueEx(key, "GOBOL_HOME", 0, winreg.REG_SZ, str(install_dir))
                winreg.SetValueEx(key, "GOBOL_INSTALL_DIR", 0, winreg.REG_SZ, str(install_dir))
                
                # 读取原类型
                try:
                    current_path, path_type = winreg.QueryValueEx(key, "Path")
                except FileNotFoundError:
                    current_path = ""
                    path_type = winreg.REG_SZ # 如果不存在，用标准字符串
                
                # 去重
                paths = [p.strip() for p in current_path.split(";") if p.strip()]
                paths = [p for p in paths if p.lower() != bin_path.lower()]
                paths.append(bin_path)
                
                # 使用原类型写回
                winreg.SetValueEx(key, "Path", 0, path_type, ";".join(paths))
                
            print_status("Registry updated successfully.", "ok")
            broadcast_env_change()
            print_status("Environment broadcast sent. New terminals will reflect changes.", "info")
            
        except Exception as e:
            print_status(f"Failed to update registry: {e}", "error")
    
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
    
    # 1. 删目录
    if install_dir.exists():
        shutil.rmtree(install_dir)
        print_status(f"Removed {install_dir}", "ok")
    
    # 2. 清理环境变量
    bin_path = str(install_dir / "bin")
    if is_windows():
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_ALL_ACCESS) as key:
                # 删除变量
                for var in ["GOBOL_HOME", "GOBOL_INSTALL_DIR"]:
                    try:
                        winreg.DeleteValue(key, var)
                    except FileNotFoundError:
                        pass
                
                # 从 Path 中剔除
                try:
                    current_path, path_type = winreg.QueryValueEx(key, "Path")
                    paths = [p.strip() for p in current_path.split(";") if p.strip()]
                    paths = [p for p in paths if p.lower() != bin_path.lower()]
                    winreg.SetValueEx(key, "Path", 0, path_type, ";".join(paths))
                except FileNotFoundError:
                    pass
            print_status("Cleaned up Windows Registry.", "ok")
            broadcast_env_change()
        except Exception as e:
            print_status(f"Failed to clean registry: {e}", "error")
    else:
        shell = os.environ.get("SHELL", "")
        rc = Path.home() / (".zshrc" if "zsh" in shell else ".bashrc")
        if rc.exists():
            rc_text = rc.read_text()
            lines = rc_text.splitlines()
            new_lines = []
            for line in lines:
                stripped = line.strip()
                if stripped == MARKER: continue
                if stripped.startswith("export GOBOL_HOME="): continue
                if stripped.startswith("export GOBOL_INSTALL_DIR="): continue
                if stripped.startswith("export PATH=") and "GOBOL_HOME" in line: continue
                new_lines.append(line)
            with open(rc, "w") as f:
                f.write("\n".join(new_lines) + "\n")
            print_status(f"Cleaned up {rc}", "ok")

    print_status("Uninstallation complete.", "ok")
    if pause:
        input("Press Enter to return to main menu...")

def task_extension_guide(pause=True):
    if pause:
        clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.OKCYAN}{Colors.BOLD}   VS Code Extension Guide   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print()

    project_root = Path(__file__).resolve().parent
    vscode_ext_path = project_root / "vscode-gobol"

    print(f"{Colors.BOLD}{Colors.OKGREEN}┌─ VS Code Extension{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│  Location: {vscode_ext_path}{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│  {Colors.BOLD}Install:{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    Option A (Marketplace): Search \"Gobol\" in VS Code Extensions{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│    Option B (Build from source):{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      cd {vscode_ext_path}{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      npm install{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      npm run build{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      npm install -g @vscode/vsce{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      vsce package{Colors.ENDC}")
    print(f"{Colors.OKGREEN}│      code --install-extension ./vscode-gobol-*.vsix{Colors.ENDC}")
    print(f"{Colors.OKGREEN}└─{Colors.ENDC}")
    if pause:
        input("Press Enter to return to main menu...")

# ==================== CLI and Main TUI Loop ====================

def parse_args():
    parser = argparse.ArgumentParser(description="Build and install the Gobol toolchain.")
    parser.add_argument("command", nargs="?", choices=("install", "uninstall", "extensions"), help="run a task without opening the interactive menu")
    parser.add_argument("--install-dir", metavar="PATH", help="installation directory (also honored by GOBOL_INSTALL_DIR)")
    parser.add_argument("--no-build", action="store_true", help="install existing release binaries without running Cargo")
    parser.add_argument("--yes", action="store_true", help="confirm destructive actions without prompting")
    parser.add_argument("--non-interactive", "--ci", dest="non_interactive", action="store_true", help="never open the TUI or prompt; defaults to install")
    return parser.parse_args()

def run_cli(args):
    command = args.command or "install"
    if command == "install":
        ok = task_build_and_install(no_build=args.no_build, install_dir=args.install_dir, pause=False)
        return 0 if ok else 1
    if command == "uninstall":
        if not args.yes:
            print_status("uninstall requires --yes in non-interactive mode.", "fail")
            return 2
        task_uninstall(install_dir=args.install_dir, assume_yes=True, pause=False)
        return 0
    task_extension_guide(pause=False)
    return 0

def main():
    args = parse_args()
    if args.command or args.non_interactive or args.install_dir or args.no_build or args.yes:
        return run_cli(args)

    while True:
        options = ["Build & Install Gobol", "Extension Guide (VS Code)", "Uninstall Gobol", "Exit"]
        choice = choose_menu("Gobol Installer", options, footer=f"GOBOL_INSTALL_DIR: {gobol_home()}")

        if choice == -1: break
        elif choice == 0: task_build_and_install()
        elif choice == 1: task_extension_guide()
        elif choice == 2: task_uninstall()
        elif choice == 3:
            print(f"{Colors.OKCYAN}Goodbye!{Colors.ENDC}")
            break
        else:
            print_status("Invalid choice.", "warn")
            time.sleep(1)

if __name__ == "__main__":
    sys.exit(main())
