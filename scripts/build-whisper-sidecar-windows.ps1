param(
  [string]$Target = "x86_64-pc-windows-msvc"
)
$ErrorActionPreference = "Stop"
$Commit = "f049fff95a089aa9969deb009cdd4892b3e74916"
$Repo = Split-Path -Parent $PSScriptRoot
$Work = if ($env:MYNA_PLAYER_WHISPER_BUILD_DIR) { $env:MYNA_PLAYER_WHISPER_BUILD_DIR } else { Join-Path $env:TEMP "myna-player-whisper-$Target" }
$Source = Join-Path $Work "source"
$Build = Join-Path $Work "build"
$Output = Join-Path $Repo "src-tauri/binaries/whisper-server-$Target.exe"
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Work | Out-Null
git clone --filter=blob:none --no-checkout https://github.com/ggml-org/whisper.cpp.git $Source
git -C $Source checkout --detach $Commit
$Actual = (git -C $Source rev-parse HEAD).Trim()
if ($Actual -ne $Commit) { throw "Unexpected whisper.cpp commit: $Actual" }
cmake -S $Source -B $Build -A x64 `
  -DCMAKE_BUILD_TYPE=Release `
  -DBUILD_SHARED_LIBS=OFF `
  -DWHISPER_BUILD_TESTS=OFF `
  -DWHISPER_BUILD_EXAMPLES=ON `
  -DWHISPER_BUILD_SERVER=ON `
  -DGGML_NATIVE=OFF `
  -DGGML_BACKEND_DL=OFF `
  -DGGML_BLAS=OFF `
  -DGGML_OPENMP=OFF
cmake --build $Build --config Release --target whisper-server --parallel
$Binary = Get-ChildItem -Path $Build -Recurse -Filter whisper-server.exe | Select-Object -First 1
if (-not $Binary) { throw "whisper-server.exe build output not found" }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Output) | Out-Null
Copy-Item -Force $Binary.FullName $Output
Write-Host "Built pinned whisper sidecar: $Output"
