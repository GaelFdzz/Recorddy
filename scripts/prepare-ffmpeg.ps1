# Prepara una copia portable de FFmpeg durante la compilación de Recorddy.
# No instala nada en Windows, no modifica PATH y reutiliza el binario en target/.
param([Parameter(Mandatory=$true)][string]$ProfileDir)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$profilePath = [IO.Path]::GetFullPath($ProfileDir)
$targetRoot = Split-Path -Parent $profilePath
$cache = Join-Path $targetRoot 'ffmpeg-portable'
$destination = Join-Path $profilePath 'tools'
$exe = Join-Path $destination 'ffmpeg.exe'
if (Test-Path $exe -PathType Leaf) { exit 0 }
New-Item -ItemType Directory -Force -Path $cache, $destination | Out-Null
$cachedExe = Join-Path $cache 'ffmpeg.exe'
$cachedLicense = Join-Path $cache 'FFMPEG-LICENSE.txt'
if (!(Test-Path $cachedExe -PathType Leaf)) {
    $url = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip'
    $hashUrl = "$url.sha256"
    $archive = Join-Path $cache 'ffmpeg-release-essentials.zip'
    $extractDir = Join-Path $cache 'extracted'
    Write-Host 'Recorddy: descargando FFmpeg portable (solo la primera vez)...'
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
    # Comprueba integridad con el SHA256 publicado por el distribuidor.
    $checksum = (Invoke-WebRequest -Uri $hashUrl -UseBasicParsing).Content
    $match = [regex]::Match([string]$checksum, '[a-fA-F0-9]{64}')
    if (!$match.Success) { throw 'No se pudo leer el SHA256 de FFmpeg' }
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($actual -ine $match.Value) { throw 'El SHA256 de FFmpeg no coincide: archivo rechazado' }
    Expand-Archive -LiteralPath $archive -DestinationPath $extractDir -Force
    $found = Get-ChildItem -LiteralPath $extractDir -Filter 'ffmpeg.exe' -Recurse -File | Select-Object -First 1
    if (!$found) { throw 'FFmpeg descargado no contiene ffmpeg.exe' }
    Copy-Item -LiteralPath $found.FullName -Destination $cachedExe -Force
    $license = Get-ChildItem -LiteralPath $extractDir -File -Recurse | Where-Object { $_.Name -in @('LICENSE','LICENSE.txt','COPYING.GPLv3','COPYING.GPLv3.txt') } | Select-Object -First 1
    if ($license) { Copy-Item -LiteralPath $license.FullName -Destination $cachedLicense -Force }
    Remove-Item -LiteralPath $archive -Force
    Remove-Item -LiteralPath $extractDir -Recurse -Force
}
Copy-Item -LiteralPath $cachedExe -Destination $exe -Force
if (Test-Path $cachedLicense) { Copy-Item -LiteralPath $cachedLicense -Destination (Join-Path $destination 'FFMPEG-LICENSE.txt') -Force }
