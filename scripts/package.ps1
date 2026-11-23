# Genera un paquete portable autocontenido; se distribuye toda la carpeta.
param([switch]$UseExistingBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $profileDir = Join-Path $root 'target/release'
    if (!$UseExistingBuild) {
        # Preparar FFmpeg es un paso de distribución, no de compilación.
        # El script cachea el archivo descargado para siguientes empaquetados.
        & (Join-Path $root 'scripts/prepare-ffmpeg.ps1') -ProfileDir $profileDir
        if (!(Test-Path (Join-Path $profileDir 'tools/ffmpeg.exe') -PathType Leaf)) { throw 'No se pudo preparar FFmpeg portable para el paquete' }
        & (Join-Path $root 'scripts/prepare-rife.ps1') -ProfileDir $profileDir
        if (!(Test-Path (Join-Path $profileDir 'tools/rife/rife-ncnn-vulkan.exe') -PathType Leaf)) { throw 'No se pudo preparar RIFE portable para el paquete' }
        python (Join-Path $root 'scripts/prepare-rife-stream.py') --profile-dir $profileDir
        if ($LASTEXITCODE -ne 0) { throw 'Falló la preparación del auxiliar RIFE en memoria' }
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Falló cargo build --release' }
    }
    $exe = Join-Path $root 'target/release/recorddy.exe'
    # El paquete usa una carpeta, no solo el .exe.
    $ffmpeg = Join-Path $root 'target/release/tools/ffmpeg.exe'
    $required = @('recorddy.exe', 'tools/ffmpeg.exe', 'tools/FFMPEG-LICENSE.txt',
        'tools/rife/rife-ncnn-vulkan.exe', 'tools/rife/recorddy-rife-stream.exe',
        'tools/rife/vcomp140.dll', 'tools/rife/LICENSE',
        'tools/rife/recorddy-rife-stream-sources.json',
        'tools/rife/recorddy-stream-rife-LICENSE.txt', 'tools/rife/recorddy-stream-ncnn-LICENSE.txt',
        'tools/rife/recorddy-stream-glslang-LICENSE.txt', 'tools/rife/recorddy-stream-vulkan-headers-LICENSE.txt')
    foreach ($file in $required) {
        if (!(Test-Path -LiteralPath (Join-Path $profileDir $file) -PathType Leaf)) { throw "Falta un archivo del portable: $file" }
    }
    $model = @('tools/rife/rife-v4.6', 'tools/rife/models/rife-v4.6') | Where-Object {
        (Test-Path -LiteralPath (Join-Path $profileDir "$_/flownet.param") -PathType Leaf) -and
        (Test-Path -LiteralPath (Join-Path $profileDir "$_/flownet.bin") -PathType Leaf)
    }
    if (!$model) { throw 'Falta el modelo RIFE v4.6 completo' }
    $distRoot = [IO.Path]::GetFullPath((Join-Path $root 'dist'))
    $output = [IO.Path]::GetFullPath((Join-Path $distRoot 'Recorddy'))
    if ([IO.Path]::GetDirectoryName($distRoot) -ne [IO.Path]::GetFullPath($root) -or
        [IO.Path]::GetDirectoryName($output) -ne $distRoot) { throw 'El destino está fuera de dist/Recorddy' }
    if (Test-Path -LiteralPath $distRoot) {
        if ((Get-Item -LiteralPath $distRoot -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'dist no puede ser un enlace' }
    }
    if (Test-Path -LiteralPath $output) {
        $items = @(Get-Item -LiteralPath $output -Force) + @(Get-ChildItem -LiteralPath $output -Recurse -Force)
        $links = $items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }
        if ($links) { throw 'El paquete existente contiene enlaces; no se elimina automáticamente' }
        Remove-Item -LiteralPath $output -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $output 'tools') | Out-Null
    Copy-Item $exe (Join-Path $output 'Recorddy.exe')
    Copy-Item $ffmpeg (Join-Path $output 'tools/ffmpeg.exe')
    Copy-Item -LiteralPath (Join-Path $profileDir 'tools/rife') -Destination (Join-Path $output 'tools/rife') -Recurse -Force
    $lic = Join-Path $root 'target/release/tools/FFMPEG-LICENSE.txt'
    if (Test-Path $lic) { Copy-Item $lic (Join-Path $output 'tools/FFMPEG-LICENSE.txt') }
    foreach ($file in @('LICENSE', 'README.md', 'NOTICE.md', 'FFMPEG-NOTICE.txt')) {
        Copy-Item -LiteralPath (Join-Path $root $file) -Destination (Join-Path $output $file)
    }
    Copy-Item (Join-Path $root 'FFMPEG-NOTICE.txt') (Join-Path $output 'tools/FFMPEG-NOTICE.txt')
    Write-Host "Paquete portable: $output"
} finally { Pop-Location }
