param(
  [string]$Architecture = "x86_64"
)
$ErrorActionPreference = "Stop"
if ($Architecture -notin @("x86_64", "x64", "amd64")) {
  throw "Unsupported Windows architecture: $Architecture"
}
$Version = "3.0.21"
$ExpectedSha256 = "a0b7ec02b50adf6417eed014fb8df50af39690505a4225b85b3dc2ed17d14843"
$Repo = Split-Path -Parent $PSScriptRoot
$Cache = if ($env:MYNA_PLAYER_LIBVLC_CACHE) { $env:MYNA_PLAYER_LIBVLC_CACHE } else { Join-Path $env:TEMP "myna-player-libvlc-cache" }
$Archive = Join-Path $Cache "vlc-$Version-win64.zip"
$Url = "https://get.videolan.org/vlc/$Version/win64/vlc-$Version-win64.zip"
$Extract = Join-Path $env:TEMP "myna-player-vlc-$Version-win64"
$Stage = Join-Path $Repo "src-tauri/vendor/libvlc"
New-Item -ItemType Directory -Force -Path $Cache | Out-Null
if (-not (Test-Path $Archive)) {
  Invoke-WebRequest -Uri $Url -OutFile "$Archive.part"
  Move-Item -Force "$Archive.part" $Archive
}
$Actual = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
if ($Actual -ne $ExpectedSha256) {
  throw "libVLC checksum mismatch. Expected $ExpectedSha256, got $Actual"
}
Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Expand-Archive -Path $Archive -DestinationPath $Extract
$Root = Join-Path $Extract "vlc-$Version"
if (-not (Test-Path (Join-Path $Root "libvlc.dll"))) {
  throw "Verified VLC archive does not contain libvlc.dll"
}
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "lib") | Out-Null
Copy-Item (Join-Path $Root "libvlc.dll") (Join-Path $Stage "lib/libvlc.dll")
Copy-Item (Join-Path $Root "libvlccore.dll") (Join-Path $Stage "lib/libvlccore.dll")
Copy-Item -Recurse (Join-Path $Root "plugins") (Join-Path $Stage "plugins")
Copy-Item (Join-Path $Root "COPYING.txt") (Join-Path $Stage "VLC-COPYING.txt")
Write-Host "Staged verified libVLC $Version win64 in $Stage"
