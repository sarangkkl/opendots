#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-install}"
case "$mode" in install|--build-only|--check) ;; *) echo 'Usage: install-from-source.sh [--check|--build-only]' >&2; exit 2;; esac
for tool in node npm cargo rustc pkg-config c++; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing $tool. See docs/BUILDING.md." >&2; exit 1; }
done
node -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<22||(a===22&&b<12)){console.error("Node 22.12+ required");process.exit(1)}'
platform="$(uname -s)"
case "$platform" in
  Darwin)
    xcode-select -p >/dev/null 2>&1 || { echo 'Install Xcode tools: xcode-select --install' >&2; exit 1; }
    bundles=app
    ;;
  Linux)
    [ "$(uname -m)" = x86_64 ] || { echo 'Linux builds are currently validated for x64 only.' >&2; exit 1; }
    command -v pkg-config >/dev/null || { echo 'Install pkg-config and the packages in docs/BUILDING.md.' >&2; exit 1; }
    for library in webkit2gtk-4.1 gtk+-3.0 dbus-1 openssl; do
      pkg-config --exists "$library" || { echo "Missing development library: $library. See docs/BUILDING.md." >&2; exit 1; }
    done
    bundles=appimage,deb
    ;;
  *) echo 'Use install-from-source.ps1 on Windows.' >&2; exit 1;;
esac
pkg-config --exists vpx || { echo 'Missing libvpx development library. macOS: brew install libvpx pkg-config llvm. Debian/Ubuntu: apt install libvpx-dev libclang-dev.' >&2; exit 1; }
vpx_libdir="$(pkg-config --variable=libdir vpx)"
[ -f "$vpx_libdir/libvpx.a" ] || { echo "Static libvpx not found in $vpx_libdir" >&2; exit 1; }
export VPX_STATIC=1
if [ "$platform" = Darwin ] && command -v brew >/dev/null && [ -z "${LIBCLANG_PATH:-}" ]; then
  llvm_root="$(brew --prefix llvm 2>/dev/null || true)"
  if [ -d "$llvm_root/lib" ]; then export LIBCLANG_PATH="$llvm_root/lib"; fi
fi
if [ "$mode" = --check ]; then echo 'Build prerequisites found.'; exit 0; fi
npm ci
npm run tauri -- build --bundles "$bundles" -- --locked
if [ "$mode" = --build-only ]; then echo 'Bundles are in src-tauri/target/release/bundle/'; exit 0; fi
if [ "$platform" = Darwin ]; then
  target="$HOME/Applications/opendots.app"
  mkdir -p "$HOME/Applications"
  if [ -e "$target" ]; then
    echo "Already installed at $target. Quit OpenDots and move the old app aside before installing this build." >&2
    exit 1
  fi
  ditto src-tauri/target/release/bundle/macos/opendots.app "$target"
  echo "Installed: $target"
else
  shopt -s nullglob
  images=(src-tauri/target/release/bundle/appimage/*.AppImage)
  [ "${#images[@]}" -eq 1 ] || { echo 'Expected exactly one AppImage. Inspect the bundle directory.' >&2; exit 1; }
  install_dir="$HOME/.local/lib/opendots"
  mkdir -p "$install_dir" "$HOME/.local/bin" "$HOME/.local/share/applications"
  if [ -e "$install_dir/OpenDots.AppImage" ]; then
    echo 'An installation already exists. Quit OpenDots and move the old AppImage aside first.' >&2
    exit 1
  fi
  install -m 755 "${images[0]}" "$install_dir/OpenDots.AppImage"
  ln -s "$install_dir/OpenDots.AppImage" "$HOME/.local/bin/opendots"
  cat > "$HOME/.local/share/applications/opendots.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=OpenDots
Exec="$install_dir/OpenDots.AppImage"
Terminal=false
Categories=Office;Utility;
DESKTOP
  echo "Installed: $install_dir/OpenDots.AppImage"
  echo 'Launch from your applications menu or ~/.local/bin/opendots. AppImage may require libfuse2/libfuse2t64.'
fi
