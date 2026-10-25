# Build OpenDots from source

Build on the target OS. Native macOS builds use the host architecture; the CI matrix covers Apple Silicon and Intel separately. Windows and Linux builds target x64. Mobile and Windows/Linux ARM are outside this release.

## Shared prerequisites

- Git, Node.js 22.12 or newer, and npm.
- A current stable Rust toolchain installed through [rustup](https://rustup.rs/).
- Platform dependencies below, following [Tauri's prerequisite guide](https://v2.tauri.app/start/prerequisites/).

Chrome/Chromium is separately installed for browser capabilities, and is not bundled. Video builds require libvpx development files, a C++ compiler and libclang to generate bindings; these are build-time dependencies.

Provider CLIs are optional for building, required to run their agents, and are never bundled. Install/login separately using [the provider guide](PROVIDERS.md).

## macOS: Apple Silicon or Intel

```sh
xcode-select --install
brew install libvpx pkg-config llvm
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
```

Install Node from nodejs.org or your package manager and Rust through rustup. From the source checkout:

```sh
bash scripts/install-from-source.sh --check
bash scripts/install-from-source.sh
```

The script runs `npm ci` and a locked Tauri release build, then copies the app to `~/Applications/opendots.app`. It refuses to overwrite an existing install; quit and move the old app aside before upgrading. User data is stored separately and retained.

Build without installing:

```sh
bash scripts/install-from-source.sh --build-only
```

Output: `src-tauri/target/release/bundle/macos/opendots.app`. Source builds are unsigned/not notarized; release signing is not configured.

## Windows x64

Install Git, Node 22.12+, Rust's MSVC toolchain, and Visual Studio 2022 Build Tools with **Desktop development with C++** (MSVC x64/x86 tools and Windows SDK). Install Microsoft Edge WebView2 Runtime if it is not already present.

Install LLVM (including libclang), and use a vcpkg checkout to install the static codec:

```powershell
$env:VCPKG_ROOT = 'C:\src\vcpkg'
& "$env:VCPKG_ROOT/vcpkg.exe" install libvpx:x64-windows-static-md
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
```

The installer sets the codec include/library paths from `VCPKG_ROOT`. Advanced builds can instead set `VPX_LIB_DIR`, `VPX_INCLUDE_DIR`, and `VPX_VERSION` explicitly. `VPX_STATIC=1` is set in Cargo configuration. libwebm builds from the Cargo dependency's bundled C++ source. Neither library is an executable sidecar.

From PowerShell in the source checkout:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-from-source.ps1 -Check
powershell -ExecutionPolicy Bypass -File scripts/install-from-source.ps1
```

The script builds an NSIS setup executable and opens the installer. To build without installing:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-from-source.ps1 -BuildOnly
```

Output: `src-tauri/target/release/bundle/nsis/*-setup.exe`. Run builds in a native Windows terminal, not WSL.

## Linux x64: Ubuntu 24.04 / Debian with WebKitGTK 4.1

```sh
sudo apt update
sudo apt install -y build-essential pkg-config curl wget file \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev librsvg2-dev \
  libayatana-appindicator3-dev libdbus-1-dev patchelf libvpx-dev libclang-dev
```

Install Node 22.12+ and Rust, then:

```sh
bash scripts/install-from-source.sh --check
bash scripts/install-from-source.sh
```

The script builds AppImage and Debian packages, installs the AppImage under `~/.local/lib/opendots`, creates `~/.local/bin/opendots`, and adds an application-menu entry. Existing installs are not overwritten. AppImage may require `libfuse2t64` (Ubuntu 24.04) or `libfuse2` on older distributions. Alternatively, install the `.deb` with your package manager.

```sh
bash scripts/install-from-source.sh --build-only
```

Output: `src-tauri/target/release/bundle/appimage/` and `bundle/deb/`. Credential storage requires an unlocked Secret Service (for example GNOME Keyring). Headless tests do not require storing credentials. Other Linux distributions are unverified.

## Reproducibility and release validation

Commit both `package-lock.json` and `src-tauri/Cargo.lock`. Install scripts use `npm ci` and `tauri build -- --locked`; they do not upgrade dependencies. CI checks formatting, Rust lint/tests, frontend compilation, UI tests on Linux, and builds native artifacts on four runners.

A configured CI job is not evidence that it ran. See [validation status](VALIDATION.md) for what has actually been exercised. Publishing, signing, notarization, auto-update hosting, and a public clone URL remain release-owner tasks.
