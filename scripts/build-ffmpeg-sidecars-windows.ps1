param([string]$Target = "x86_64-pc-windows-msvc")
$ErrorActionPreference = "Stop"
$Repo = Split-Path -Parent $PSScriptRoot
$Bash = "C:\msys64\usr\bin\bash.exe"
if (-not (Test-Path $Bash)) { throw "MSYS2 bash is required to build FFmpeg sidecars" }
$UnixRepo = (& $Bash -lc "cygpath -u '$($Repo.Replace("'", "''"))'").Trim()
& $Bash -lc "cd '$UnixRepo' && export PATH=/mingw64/bin:/usr/bin:`$PATH && ./scripts/build-ffmpeg-sidecars.sh '$Target'"
if ($LASTEXITCODE -ne 0) { throw "FFmpeg sidecar build failed with exit code $LASTEXITCODE" }
