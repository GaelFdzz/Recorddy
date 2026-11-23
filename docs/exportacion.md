# Exportación y Smooth Motion

## Modos y límites

- **Original:** utiliza las rutas habituales del editor. Puede copiar el contenido sin recodificar cuando el recorte y los ajustes lo permiten.
- **Social 60:** salida experimental de 60 FPS. Reconstruye muestras de fuentes lentas y aplica una corrección de exposición corta y limitada, con protección de bordes.
- **Smooth 120:** salida de 120 FPS con interpolación cuando faltan muestras nativas.

Smooth admite SDR BT.709. No es un tratamiento HDR ni una garantía de que un video se verá como una referencia de redes sociales. Una fuente de 60 FPS no contiene las mismas muestras de movimiento que una captura nativa de 120 FPS.

## Procesamiento

La ruta en memoria conecta tres procesos directos:

```text
FFmpeg (decodificar) → recorddy-rife-stream (interpolar) → FFmpeg (codificar)
```

Los cuadros viajan por tuberías BGR24 sin compresión. La ruta habitual evita las secuencias PNG y solapa las tres etapas. El transporte mantiene como máximo cuatro cuadros pendientes y dos trabajadores de RIFE; su memoria no crece con la duración completa del clip.

Se utiliza **RIFE v4.6**, sin reducir la resolución para acelerar el proceso. Los pares exactamente iguales no se interpolan; los tramos constantes no necesitan inicializar el modelo dentro del auxiliar. Las fuentes de al menos 120 FPS aprovechan muestras nativas y evitan RIFE. Las composiciones del editor pueden requerir un render H.264 intermedio: no se promete una ruta completa sin pérdidas desde el original para todas las ediciones.

Los encoders se comprueban mediante una codificación real a la resolución y FPS pedidos. Existen rutas para NVIDIA NVENC, AMD AMF, Intel QSV y CPU. Ante un fallo se intenta otra alternativa comprobada; si el auxiliar no está disponible o falla, se conserva el respaldo mediante PNG con el mismo modelo y resolución.

La interpolación dinámica sigue requiriendo Vulkan. El respaldo de codificación por CPU no proporciona interpolación por CPU. AMD e Intel todavía necesitan validación física; no basta con detectar sus encoders en FFmpeg.

La captura tiene prioridad: Smooth requiere pausarla. Cancelar o detectar una captura activa termina los hijos y limpia los temporales. El MP4 final se valida y se publica sin sobrescribir un destino existente.

## Preparar las herramientas

La aplicación busca `tools/` junto al ejecutable. `scripts/prepare-ffmpeg.ps1` prepara FFmpeg; `scripts/prepare-rife.ps1` prepara RIFE, su modelo y las DLL del bundle. Para preparar una compilación de desarrollo:

```powershell
./scripts/prepare-ffmpeg.ps1 -ProfileDir target/fast
./scripts/prepare-rife.ps1 -ProfileDir target/fast
python scripts/prepare-rife-stream.py --profile-dir target/fast
```

Para reutilizar las fuentes descargadas al compilar el auxiliar en otro perfil:

```powershell
python scripts/prepare-rife-stream.py --offline --profile-dir target/release
```

Se requieren Python 3.12+, MSVC y las herramientas CMake y Ninja de Visual Studio. Las fuentes se fijan y verifican con SHA-256 en `native/rife-stream/sources.lock.json`; la caché y el build quedan en `target/rife-stream-build/`.

El paquete final contiene el auxiliar compilado, modelos, DLL y licencias. No incluye la caché de compilación ni una DLL Vulkan tomada de la PC de desarrollo. El ZIP antiguo de RIFE aún no tiene un hash anclado en su preparador; los hashes fijados del auxiliar no verifican ese ZIP. Consultá [FFMPEG-NOTICE.txt](../FFMPEG-NOTICE.txt) para la distribución.

## Código fuente y portable

`native/rife-stream/` contiene solamente el código C++, su definición de compilación CMake y las versiones fijadas de sus dependencias. Son necesarios para reconstruir `recorddy-rife-stream.exe`; borrar estas fuentes impediría regenerar la ruta de exportación rápida.

`scripts/` conserva cuatro entradas de preparación y empaquetado:

| Script | Propósito |
| --- | --- |
| `package.ps1` | Compilar y reunir el portable; `-UseExistingBuild` reutiliza una compilación completa. |
| `prepare-ffmpeg.ps1` | Preparar FFmpeg y su licencia. |
| `prepare-rife.ps1` | Preparar el motor de respaldo y el modelo RIFE v4.6. |
| `prepare-rife-stream.py` | Descargar las fuentes verificadas y compilar el auxiliar en memoria. |

Estas carpetas no son dependencias de ejecución del portable. Para usar Recorddy, conservá únicamente el paquete completo de `dist/Recorddy/`, incluidas sus herramientas, modelos y licencias. Las descargas y compilaciones temporales se generan en `target/`, que puede eliminarse después de verificar el paquete.
