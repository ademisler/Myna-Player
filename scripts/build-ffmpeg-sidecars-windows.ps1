param([string]$Target = "x86_64-pc-windows-msvc")
$ErrorActionPreference = "Stop"
$Repo = Split-Path -Parent $PSScriptRoot
$Bash = "C:\msys64\usr\bin\bash.exe"
if (-not (Test-Path $Bash)) { throw "MSYS2 bash is required to build FFmpeg sidecars" }
if ($Repo -notmatch '^[A-Za-z]:\\') { throw "Expected an absolute Windows repository path, got $Repo" }

# Do not derive the path by capturing `cygpath` output. A fresh MSYS2 install may
# print first-run setup messages to stdout, which would corrupt the captured path.
$Drive = $Repo.Substring(0, 1).ToLowerInvariant()
$Rest = $Repo.Substring(2).Replace('\', '/')
$UnixRepo = "/$Drive$Rest"

# Complete MSYS2's first-run initialization before executing the build command.
& $Bash -lc "true" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MSYS2 initialization failed with exit code $LASTEXITCODE" }

& $Bash -lc "cd '$UnixRepo' && export PATH=/mingw64/bin:/usr/bin:`$PATH && ./scripts/build-ffmpeg-sidecars.sh '$Target'"
if ($LASTEXITCODE -ne 0) { throw "FFmpeg sidecar build failed with exit code $LASTEXITCODE" }
