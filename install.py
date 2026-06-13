#!/usr/bin/env python3
"""
Our Goal: Build the project and Copy the binary into the directory by using the env `GOBOL_INSTALL_DIR`.

1. Check if the env `GOBOL_INSTALL_DIR` is set.
   If not, use platform-specific default:
   - Windows: %USERPROFILE%\\.local\\bin
   - Linux/macOS: ~/.local/bin

2. Install multiple binaries: gobol and grape.

3. Copy ./std/ directory to current working directory (or specified location).

4. Add the installation directory to PATH on Unix-like systems (Linux/macOS):
   - Detect shell (zsh, bash, fish, etc.)
   - Write to appropriate config file
   - Prevent duplicate entries via marker comments

5. On Windows, provide instructions for manual PATH addition.

6. Support custom resource directory via `GOBOL_RESOURCE_DIR` environment variable.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple


def get_default_install_dir() -> Path:
    """Get platform-specific default installation directory."""
    if sys.platform == "win32":
        # Windows: %USERPROFILE%\.local\bin
        user_profile = Path(os.environ.get("USERPROFILE", Path.home()))
        return user_profile / ".local" / "bin" / "gobol"
    else:
        # Linux/macOS: ~/.local/bin
        return Path.home() / ".local" / "bin" / "gobol"


def get_binary_names() -> List[str]:
    """
    Get list of binary names to install.
    """
    binaries = ["gobol", "grape"]
    
    if sys.platform == "win32":
        binaries = [b + ".exe" for b in binaries]
    
    return binaries


def detect_shell_config() -> Tuple[Path, str]:
    """
    Detect the user's shell and return the appropriate config file path.
    
    Returns:
        Tuple of (config_file_path, shell_name)
    """
    shell = os.environ.get("SHELL", "")
    home = Path.home()
    
    if sys.platform == "darwin":
        # macOS default is zsh
        if "zsh" in shell:
            return home / ".zprofile", "zsh"
        elif "bash" in shell:
            return home / ".profile", "bash"
        else:
            return home / ".zprofile", "zsh"
    
    elif sys.platform.startswith("linux"):
        if "zsh" in shell:
            return home / ".zshrc", "zsh"
        elif "bash" in shell:
            return home / ".bashrc", "bash"
        elif "fish" in shell:
            return home / ".config/fish/config.fish", "fish"
        else:
            return home / ".profile", "unknown"
    
    else:
        return home / ".profile", "unknown"


def is_path_in_shell_config(config_file: Path, target_path: str, marker: str) -> bool:
    """
    Check if the target path or marker already exists in the config file.
    """
    if not config_file.exists():
        return False
    
    try:
        content = config_file.read_text(encoding='utf-8')
        if marker in content:
            return True
        if f'PATH="{target_path}:$PATH"' in content or f"PATH={target_path}:$PATH" in content:
            return True
    except (IOError, OSError) as e:
        print(f"Warning: Could not read {config_file}: {e}", file=sys.stderr)
    
    return False

def add_to_path_unix(target_dir: Path) -> bool:
    """
    Add target directory to PATH on Linux/macOS by modifying shell config.
    Also set GOBOL_INSTALL_DIR environment variable.
    """
    target_str = str(target_dir.absolute())
    
    # Check if already in PATH environment variable
    current_path = os.environ.get("PATH", "")
    
    config_file, shell_name = detect_shell_config()
    marker = "# Added by Gobol installer"
    
    # Check if GOBOL_INSTALL_DIR is already configured in shell config
    has_install_dir = False
    has_path_config = False
    
    if config_file.exists():
        try:
            content = config_file.read_text(encoding='utf-8')
            if marker in content:
                if f'GOBOL_INSTALL_DIR="{target_str}"' in content:
                    has_install_dir = True
                if f'PATH="{target_str}:$PATH"' in content or f"PATH={target_str}:$PATH" in content:
                    has_path_config = True
        except (IOError, OSError):
            pass
    
    # Write if either PATH config or GOBOL_INSTALL_DIR is missing
    if not (has_path_config and has_install_dir):
        print(f"Adding configuration to {config_file}...")
        
        # Generate shell-specific syntax with both PATH and GOBOL_INSTALL_DIR
        if shell_name == "fish":
            lines_to_add = f"""
{marker}
if test -d "{target_str}"
    # Only add to PATH if not already there
    if not contains $PATH "{target_str}"
        set -x PATH "{target_str}" $PATH
    end
    set -x GOBOL_INSTALL_DIR "{target_str}"
end
"""
        elif shell_name == "zsh" or shell_name == "bash":
            lines_to_add = f"""

{marker}
if [ -d "{target_str}" ]; then
    # Only add to PATH if not already there
    case ":$PATH:" in
        *":{target_str}:"*) ;;
        *) export PATH="{target_str}:$PATH" ;;
    esac
    export GOBOL_INSTALL_DIR="{target_str}"
fi
"""
        else:
            lines_to_add = f"""

{marker}
export PATH="{target_str}:$PATH"
export GOBOL_INSTALL_DIR="{target_str}"
"""
        
        try:
            config_file.parent.mkdir(parents=True, exist_ok=True)
            with open(config_file, 'a', encoding='utf-8') as f:
                f.write(lines_to_add)
            print(f"Successfully added to {config_file}")
            print(f"Please run 'source {config_file}' or restart your terminal to apply changes")
        except (IOError, OSError) as e:
            print(f"Warning: Could not write to {config_file}: {e}", file=sys.stderr)
            return False
    else:
        print(f"Info: Configuration already exists in {config_file}")
    
    # Set in current session for immediate use
    if target_str not in current_path.split(":"):
        os.environ["PATH"] = f"{target_str}:{current_path}"
    os.environ["GOBOL_INSTALL_DIR"] = target_str
    print(f"✓ Set GOBOL_INSTALL_DIR={target_str} for current session")
    
    return True

def add_to_path_windows(target_dir: Path) -> bool:
    """
    Add target directory to PATH on Windows using setx.
    Also set GOBOL_INSTALL_DIR environment variable.
    """
    target_str = str(target_dir.absolute())
    current_path = os.environ.get("PATH", "")
    
    # Check if already in PATH
    if target_str in current_path.split(";"):
        print(f"Info: {target_str} is already in PATH")
    else:
        # Add to PATH using setx (user scope, no admin required)
        new_path = f"{target_str};{current_path}"
        # setx truncates at 1024 chars, so only set if within limit
        if len(new_path) < 1024:
            result = subprocess.run(
                ["setx", "PATH", new_path],
                capture_output=True,
                text=True
            )
            if result.returncode == 0:
                print(f"✓ Added {target_str} to PATH")
            else:
                print(f"Warning: setx failed: {result.stderr}", file=sys.stderr)
                print(f"Please manually add {target_str} to your PATH")
        else:
            print(f"Warning: PATH too long, setx cannot update it automatically")
            print(f"Please manually add {target_str} to your PATH")
    
    # Set GOBOL_INSTALL_DIR
    result = subprocess.run(
        ["setx", "GOBOL_INSTALL_DIR", target_str],
        capture_output=True,
        text=True
    )
    if result.returncode == 0:
        print(f"✓ Set GOBOL_INSTALL_DIR={target_str}")
    else:
        print(f"Warning: Failed to set GOBOL_INSTALL_DIR: {result.stderr}", file=sys.stderr)
    
    # Update current session
    if target_str not in current_path.split(";"):
        os.environ["PATH"] = f"{target_str};{current_path}"
    os.environ["GOBOL_INSTALL_DIR"] = target_str
    
    print("\n" + "=" * 70)
    print("Please restart your terminal for changes to take effect.")
    print("=" * 70)
    
    return True

def get_resource_dir() -> Optional[Path]:
    """
    Get custom resource directory from GOBOL_RESOURCE_DIR environment variable.
    """
    resource_dir = os.environ.get("GOBOL_RESOURCE_DIR")
    if resource_dir:
        path = Path(resource_dir)
        try:
            path.mkdir(parents=True, exist_ok=True)
            return path
        except (IOError, OSError) as e:
            print(f"Warning: Could not create resource directory {path}: {e}", file=sys.stderr)
            return None
    return None


def build_project() -> bool:
    """
    Build the Rust project in release mode.
    """
    print("Building project in release mode...")
    
    try:
        result = subprocess.run(
            ["cargo", "build", "--release"],
            capture_output=True,
            text=True,
            check=False
        )
        
        if result.returncode != 0:
            print("Build failed!", file=sys.stderr)
            if result.stderr:
                print(result.stderr, file=sys.stderr)
            return False
        
        print("Build successful")
        return True
        
    except FileNotFoundError:
        print("Error: 'cargo' command not found. Please ensure Rust is installed.", file=sys.stderr)
        return False
    except subprocess.SubprocessError as e:
        print(f"Error during build: {e}", file=sys.stderr)
        return False


def copy_binaries(target_dir: Path) -> bool:
    """
    Copy all built binaries to target directory.
    
    Returns:
        True if all copies successful, False otherwise
    """
    binary_names = get_binary_names()
    success_count = 0
    
    for binary_name in binary_names:
        source = Path("target/release") / binary_name
        
        if not source.exists():
            print(f"Warning: Binary not found at {source}, skipping...", file=sys.stderr)
            continue
        
        try:
            dest = target_dir / binary_name
            shutil.copy2(source, dest)
            print(f"Copied: {source} -> {dest}")
            
            # Add executable permission on Unix-like systems
            if sys.platform != "win32":
                current_mode = dest.stat().st_mode
                dest.chmod(current_mode | 0o755)
            
            success_count += 1
            
        except (IOError, OSError, shutil.Error) as e:
            print(f"Error copying {binary_name}: {e}", file=sys.stderr)
            return False
    
    if success_count == 0:
        print("Error: No binaries were copied", file=sys.stderr)
        return False
    
    if success_count < len(binary_names):
        print(f"Warning: Only {success_count}/{len(binary_names)} binaries were copied")
    
    if sys.platform != "win32":
        print("Added executable permission to all binaries")
    
    return True


def copy_std_directory(target_dir: Path) -> bool:
    """
    Copy ./std/ directory to target directory.
    If ./std/ already exists, merge contents (overwrite existing files).
    
    Returns:
        True if successful, False otherwise
    """
    source_std = Path("./std")
    
    if not source_std.exists():
        print("Warning: ./std/ directory not found, skipping...", file=sys.stderr)
        return True
    
    if not source_std.is_dir():
        print("Warning: ./std is not a directory, skipping...", file=sys.stderr)
        return True
    
    target_std = target_dir / "std"
    
    print(f"Copying ./std/ to {target_std}...")
    
    try:
        if target_std.exists():
            # Merge contents
            for item in source_std.iterdir():
                dest = target_std / item.name
                if item.is_dir():
                    if dest.exists():
                        shutil.copytree(item, dest, dirs_exist_ok=True)
                    else:
                        shutil.copytree(item, dest)
                else:
                    shutil.copy2(item, dest)
            print(f"Merged ./std/ contents into {target_std}")
        else:
            # Simple copy
            shutil.copytree(source_std, target_std)
            print(f"Copied ./std/ to {target_std}")
        
        return True
        
    except (IOError, OSError, shutil.Error) as e:
        print(f"Error copying ./std/ directory: {e}", file=sys.stderr)
        return False


def copy_resources(resource_dir: Optional[Path]) -> bool:
    """
    Copy additional resource files if resource directory is configured.
    """
    if resource_dir is None:
        return True
    
    source_resources = Path("resources")
    
    if not source_resources.exists():
        return True
    
    print(f"Copying resources to {resource_dir}...")
    
    try:
        for item in source_resources.iterdir():
            dest = resource_dir / item.name
            if item.is_dir():
                shutil.copytree(item, dest, dirs_exist_ok=True)
            else:
                shutil.copy2(item, dest)
        
        print("Resources copied successfully")
        return True
        
    except (IOError, OSError, shutil.Error) as e:
        print(f"Warning: Could not copy resources: {e}", file=sys.stderr)
        return False


def main() -> None:
    """
    Main installation routine.
    """
    print("=" * 60)
    print("Gobol Installer")
    print("=" * 60)
    
    # Step 1: Determine installation directory
    install_dir = os.environ.get("GOBOL_INSTALL_DIR")
    
    if not install_dir:
        install_dir = get_default_install_dir()
        print(f"GOBOL_INSTALL_DIR not set, using default: {install_dir}")
    else:
        print(f"Using GOBOL_INSTALL_DIR: {install_dir}")
    
    target_dir = Path(install_dir)
    
    try:
        target_dir.mkdir(parents=True, exist_ok=True)
        print(f"Installation directory ready: {target_dir}")
    except (IOError, OSError) as e:
        print(f"Error: Cannot create installation directory {target_dir}: {e}", file=sys.stderr)
        sys.exit(1)
    
    # Step 2: Get resource directory (optional)
    resource_dir = get_resource_dir()
    if resource_dir:
        print(f"Using GOBOL_RESOURCE_DIR: {resource_dir}")
    
    # Step 3: Build the project
    if not build_project():
        sys.exit(1)
    
    # Step 4: Copy binaries
    if not copy_binaries(target_dir):
        sys.exit(1)
    
    # Step 5: Copy ./std/ directory to installation directory
    copy_std_directory(target_dir)
    
    # Step 6: Copy resources (if configured)
    copy_resources(resource_dir)
    
    # Step 7: Handle PATH configuration
    if sys.platform == "win32":
        add_to_path_windows(target_dir)
    else:
        add_to_path_unix(target_dir)
    
    # Step 8: Final success message
    binary_names = get_binary_names()
    print(f"\nInstallation complete.")
    print(f"Binaries installed to: {target_dir}")
    for name in binary_names:
        print(f"  - {target_dir / name}")
    print(f"Standard library copied to: {target_dir / 'std'}")


if __name__ == "__main__":
    main()
