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
    if override := os.environ.get("GOBOL_INSTALL_DIR"):
        return Path(override)
    if override := os.environ.get("GOBOL_HOME"):
        return Path(override)
    return Path.home() / ".gobol"

# ==================== TUI Task Functions ====================

def task_build_and_install(no_build=False):
    """Build and install the Gobol toolchain with user-defined installation directory."""
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
    
    print(f"\n{Colors.OKCYAN}Current installation directory: {Colors.ENDC}{default_install_dir}")
    user_input = input(f"{Colors.OKCYAN}Enter new installation directory (or press Enter to keep current): {Colors.ENDC}").strip()
    
    if user_input:
        install_dir = Path(user_input).expanduser().resolve()
    else:
        install_dir = default_install_dir
    
    print_status(f"Installation directory set to: {install_dir}", "ok")
    
    # ----- 构建 -----
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
    
    # ----- 安装 -----
    install_dir.mkdir(parents=True, exist_ok=True)
    (install_dir / "bin").mkdir(parents=True, exist_ok=True)
    (install_dir / "lib").mkdir(parents=True, exist_ok=True)
    
    suffix = ".exe" if is_windows() else ""
    binaries = [f"gobol{suffix}", f"grape{suffix}", f"gobol-lsp{suffix}"]
    for name in binaries:
        src = Path("target/release") / name
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
    src_std = Path("std")
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
    input("Press Enter to return to main menu...")

def task_uninstall():
    clear_screen()
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print(f"{Colors.FAIL}{Colors.BOLD}   Uninstall Gobol   {Colors.ENDC}")
    print(f"{Colors.HEADER}{'=' * 60}{Colors.ENDC}")
    print("\nWarning: This will permanently delete the Gobol installation directory.")
    install_dir = gobol_home()
    print(f"Installation directory: {install_dir}")
    confirm = input(f"{Colors.FAIL}Confirm uninstall? (type 'yes' to confirm): {Colors.ENDC}")
    if confirm.lower() != "yes":
        print_status("Uninstall cancelled.", "info")
        input("Press Enter to return to main menu...")
        return
    
    if install_dir.exists():
        shutil.rmtree(install_dir)
        print_status(f"Removed {install_dir}", "ok")
        print_status("Please manually clean up your shell PATH in .bashrc/.zshrc", "warn")
    else:
        print_status("No installation found.", "warn")
    input("Press Enter to return to main menu...")

def task_extension_guide():
    """显示 VS Code 和 Neovim 扩展安装指南（跨平台命令）"""
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

    input(f"{Colors.GREY}Press Enter to return to main menu...{Colors.ENDC}")

# ==================== Main TUI Loop ====================

def main():
    while True:
        options = [
            "Build & Install Gobol",
            "Extension Guide (VS Code & Neovim)",
            "Uninstall Gobol",
            "Exit"
        ]
        print_menu(
            "Gobol Installer",
            options,
            footer=f"GOBOL_INSTALL_DIR: {gobol_home()}"
        )
        choice = input(f"{Colors.OKCYAN}❯ {Colors.ENDC}").strip().lower()

        if choice == "q":
            break
        elif choice == "1":
            task_build_and_install()
        elif choice == "2":
            task_extension_guide()
        elif choice == "3":
            task_uninstall()
        elif choice == "4" or choice == "q":
            print(f"{Colors.OKCYAN}Goodbye!{Colors.ENDC}")
            break
        else:
            print_status("Invalid choice.", "warn")
            time.sleep(1)

if __name__ == "__main__":
    main()
