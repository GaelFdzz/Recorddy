# Descarga RIFE-ncnn-vulkan portátil en el perfil solicitado. No instala nada en Windows.
# Versión fija para reproducibilidad. Se recomienda fijar SHA-256 antes de distribuir al público.
param([Parameter(Mandatory=$true)][string]$ProfileDir)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$profilePath = [IO.Path]::GetFullPath($ProfileDir)
$targetRoot = Split-Path -Parent $profilePath
$cache = Join-Path $targetRoot 'rife-portable-20221029'
$destination = Join-Path $profilePath 'tools/rife'
$exe = Join-Path $destination 'rife-ncnn-vulkan.exe'
# Smooth Social depende de la malla temporal de v4.6. No sustituir el modelo silenciosamente.
$modelNames = @('rife-v4.6')
function Find-RifeModel([string]$folder) {
    foreach ($name in $modelNames) {
        foreach ($relative in @("models/$name", $name)) {
            $candidate = Join-Path $folder $relative
            if ((Test-Path -LiteralPath (Join-Path $candidate 'flownet.param') -PathType Leaf) -and
                (Test-Path -LiteralPath (Join-Path $candidate 'flownet.bin') -PathType Leaf)) { return $candidate }
        }
    }
    return $null
}
if ((Test-Path $exe -PathType Leaf) -and (Find-RifeModel $destination)) {
    Write-Host "RIFE ya está preparado en $destination"
    exit 0
}
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$archive = Join-Path $cache 'rife-ncnn-vulkan-20221029-windows.zip'
$extract = Join-Path $cache 'extracted'
$url = 'https://github.com/nihui/rife-ncnn-vulkan/releases/download/20221029/rife-ncnn-vulkan-20221029-windows.zip'
if (!(Test-Path $archive -PathType Leaf)) {
    Write-Host 'Recorddy: descargando RIFE-ncnn-vulkan (una sola vez)...'
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
}
# El archivo ZIP está fijado por versión y origen, pero esta versión experimental
# aún NO tiene un SHA-256 anclado. No publicar como distribución verificada.
if (Test-Path $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
$found = Get-ChildItem -LiteralPath $extract -Filter 'rife-ncnn-vulkan.exe' -File -Recurse | Select-Object -First 1
if (!$found) { throw 'El ZIP no contiene rife-ncnn-vulkan.exe' }
$bundle = Split-Path -Parent $found.FullName
$sourceModel = Find-RifeModel $bundle
if (!$sourceModel) {
    $available = (Get-ChildItem -LiteralPath $bundle -Directory -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'rife*' } | Select-Object -ExpandProperty FullName) -join ', '
    throw "El paquete no contiene un modelo RIFE compatible. Modelos detectados: $available"
}
Write-Host "Recorddy: modelo detectado: $(Split-Path -Leaf $sourceModel)"
New-Item -ItemType Directory -Force -Path $destination | Out-Null
# Copiar el bundle completo: DLL, modelos y licencias de terceros.
Copy-Item -Path (Join-Path $bundle '*') -Destination $destination -Recurse -Force
if (!(Test-Path $exe -PathType Leaf) -or !(Find-RifeModel $destination)) {
    throw 'RIFE no quedó instalado en el perfil solicitado'
}
Remove-Item -LiteralPath $extract -Recurse -Force
Write-Host "RIFE preparado en $destination"
