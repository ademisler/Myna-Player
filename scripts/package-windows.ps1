$ErrorActionPreference = "Stop"
$Repo = Split-Path -Parent $PSScriptRoot
Set-Location $Repo
& "$PSScriptRoot/stage-libvlc-windows.ps1" x86_64
& "$PSScriptRoot/build-whisper-sidecar-windows.ps1" x86_64-pc-windows-msvc
& "$PSScriptRoot/build-ffmpeg-sidecars-windows.ps1" x86_64-pc-windows-msvc
trunk build --release
cargo tauri build --target x86_64-pc-windows-msvc --config src-tauri/tauri.bundle.conf.json
