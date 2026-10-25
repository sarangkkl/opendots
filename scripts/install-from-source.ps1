param([switch]$BuildOnly, [switch]$Check)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'The Windows source installer currently supports x64.' }
foreach ($tool in @('node','npm','cargo','rustc')) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing $tool. See docs/BUILDING.md." }
}
node -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<22||(a===22&&b<12))process.exit(1)'
if ($LASTEXITCODE -ne 0) { throw 'Node 22.12+ required.' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++.' }
$cpp = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $cpp) { throw 'MSVC x64 C++ tools are missing. See docs/BUILDING.md.' }
if (-not $env:VPX_LIB_DIR) {
  if (-not $env:VCPKG_ROOT) { throw 'Set VCPKG_ROOT and install libvpx:x64-windows-static-md, or set VPX_LIB_DIR, VPX_INCLUDE_DIR and VPX_VERSION. See docs/BUILDING.md.' }
  $env:VPX_LIB_DIR = Join-Path $env:VCPKG_ROOT 'installed/x64-windows-static-md/lib'
  $env:VPX_INCLUDE_DIR = Join-Path $env:VCPKG_ROOT 'installed/x64-windows-static-md/include'
  $env:VPX_VERSION = '1.15.2'
}
$env:VPX_STATIC = '1'
if (-not (Test-Path (Join-Path $env:VPX_LIB_DIR 'libvpx.lib'))) {
  $shortName = Join-Path $env:VPX_LIB_DIR 'vpx.lib'
  if (-not (Test-Path $shortName)) { throw 'Static libvpx.lib or vpx.lib is missing. Install libvpx:x64-windows-static-md.' }
  Copy-Item $shortName (Join-Path $env:VPX_LIB_DIR 'libvpx.lib')
}
if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM/bin' }
if (-not (Test-Path (Join-Path $env:LIBCLANG_PATH 'libclang.dll'))) { throw 'Install LLVM and set LIBCLANG_PATH to the directory containing libclang.dll.' }
if ($Check) { Write-Host 'Build prerequisites found. WebView2 is also required at runtime.'; exit 0 }
npm ci
if ($LASTEXITCODE -ne 0) { throw 'npm ci failed.' }
npm run tauri -- build --bundles nsis -- --locked
if ($LASTEXITCODE -ne 0) { throw 'Tauri build failed.' }
if ($BuildOnly) { Write-Host 'Installer: src-tauri/target/release/bundle/nsis/'; exit 0 }
$installers = @(Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one installer. Inspect the bundle directory.' }
$process = Start-Process -FilePath $installers[0].FullName -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Installer failed with exit code $($process.ExitCode)." }
Write-Host 'OpenDots installation completed.'
