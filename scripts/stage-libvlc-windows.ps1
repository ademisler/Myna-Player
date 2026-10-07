param(
  [string]$Architecture = "x86_64"
)
$ErrorActionPreference = "Stop"
if ($Architecture -notin @("x86_64", "x64", "amd64")) {
  throw "Unsupported Windows architecture: $Architecture"
}
$Version = "3.0.21"
$ExpectedSha256 = "a0b7ec02b50adf6417eed014fb8df50af39690505a4225b85b3dc2ed17d14843"
$ExpectedSize = 77665682
$Repo = Split-Path -Parent $PSScriptRoot
$Cache = if ($env:MYNA_PLAYER_LIBVLC_CACHE) { $env:MYNA_PLAYER_LIBVLC_CACHE } else { Join-Path $env:TEMP "myna-player-libvlc-cache" }
$Archive = Join-Path $Cache "vlc-$Version-win64.zip"
$Url = "https://get.videolan.org/vlc/$Version/win64/vlc-$Version-win64.zip"
$Extract = Join-Path $env:TEMP "myna-player-vlc-$Version-win64"
$Stage = Join-Path $Repo "src-tauri/vendor/libvlc"
New-Item -ItemType Directory -Force -Path $Cache | Out-Null

function Test-VerifiedArchive([string]$Path) {
  if (-not (Test-Path $Path)) { return $false }
  $file = Get-Item $Path
  if ($file.Length -ne $ExpectedSize) { return $false }
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $signature = New-Object byte[] 4
    if ($stream.Read($signature, 0, 4) -ne 4) { return $false }
    if ($signature[0] -ne 0x50 -or $signature[1] -ne 0x4B -or $signature[2] -ne 0x03 -or $signature[3] -ne 0x04) {
      return $false
    }
  } finally {
    $stream.Dispose()
  }
  $actual = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLowerInvariant()
  return $actual -eq $ExpectedSha256
}

if (-not (Test-VerifiedArchive $Archive)) {
  Remove-Item -Force $Archive, "$Archive.part" -ErrorAction SilentlyContinue
  & curl.exe --fail --location --retry 5 --retry-delay 2 --retry-all-errors --output "$Archive.part" $Url
  if ($LASTEXITCODE -ne 0) { throw "Could not download verified VLC archive from $Url" }
  Move-Item -Force "$Archive.part" $Archive
}
if (-not (Test-VerifiedArchive $Archive)) {
  $actual = if (Test-Path $Archive) { (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant() } else { "missing" }
  throw "libVLC archive verification failed. Expected SHA-256 $ExpectedSha256 and $ExpectedSize bytes, got $actual"
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
