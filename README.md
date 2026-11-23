# Recorddy

Grabador de clips y sesiones continuas para **Windows**, desarrollado en Rust. Captura juegos o el escritorio, guarda repeticiones instantáneas y permite editar y exportar los videos desde la misma aplicación.

Recorddy es un fork independiente de [Clippy](https://github.com/santiremedi/clippy). Conserva la licencia GPL-3.0-or-later y las atribuciones del proyecto original; no está afiliado a sus autores.

## Funciones

- **Repeticiones instantáneas:** guarda los últimos segundos con un atajo. La duración máxima del búfer se adapta a la RAM disponible, hasta 10 minutos.
- **Grabación continua:** graba sesiones a disco. Se elige entre este modo y las repeticiones instantáneas.
- **Captura H.264:** utiliza las APIs nativas de Windows y aceleración por hardware cuando está disponible.
- **Monitores y audio:** selección automática o fija de monitor; audio del juego, escritorio y micrófono con controles independientes.
- **Editor integrado:** biblioteca de clips, línea de tiempo, recortes y exportación MP4.
- **Smooth Motion:** exportación experimental a 60 o 120 FPS mediante RIFE, aplicada al exportar y sin modificar la captura habitual.
- **Tres idiomas:** español, inglés y portugués de Brasil.

Esta variante tiene las actualizaciones remotas desactivadas.

## Uso

Ejecutá `Recorddy.exe` para abrir la aplicación y su icono en la bandeja de Windows. Desde los ajustes, elegí la carpeta de videos, el modo de grabación y los atajos. El atajo predeterminado para guardar un clip es **F9**.

Los videos se guardan por defecto en **Videos/Recorddy**. La configuración y el registro se guardan en `%APPDATA%\Recorddy`; esta versión no guarda todos sus datos junto al ejecutable.

Al usar un paquete con Smooth Motion, conservá la carpeta `tools` junto al ejecutable. Para exportar con Smooth, pausá la captura. **Social 60 es experimental:** la interpolación puede producir artefactos y no garantiza una mejora visual en todos los clips.

## Requisitos

- Windows x64 con las APIs de captura y Media Foundation disponibles.
- Driver de video actualizado y compatibilidad con las funciones de captura utilizadas por Windows.
- Para interpolar con RIFE: GPU con Vulkan y las herramientas y el modelo del paquete completo. La codificación por CPU no reemplaza la interpolación Vulkan.

La ruta de exportación se probó físicamente con NVIDIA RTX 4060 y codificación por CPU. AMD AMF e Intel QSV tienen detección y respaldo, pero falta validarlos en equipos de esas familias.

## Compilar

Instalá Rust con el toolchain MSVC y las herramientas de C++ de Visual Studio, incluido el SDK de Windows. Desde la raíz del proyecto:

```powershell
cargo build --release --locked
```

El ejecutable queda en `target/release/recorddy.exe`. Esta compilación no descarga ni prepara automáticamente FFmpeg o RIFE: son herramientas externas que se incorporan al preparar el paquete.

Para iterar durante el desarrollo:

```powershell
cargo build --profile fast --features devtools --locked
cargo check --locked
cargo check --locked --features devtools
cargo test --locked --features devtools
```

Los modos de prueba solo están disponibles al compilar con `devtools`. Compilá en `target/` para reutilizar los perfiles y las cachés del proyecto.

## Preparar el paquete completo

Además de Rust y MSVC, necesitás **Python 3.12 o posterior**, CMake y Ninja instalados con Visual Studio. Desde PowerShell:

```powershell
./scripts/package.ps1
```

El script prepara FFmpeg, RIFE v4.6 y el auxiliar de interpolación en memoria, compila Recorddy y reúne los archivos en `dist/Recorddy/`. La primera ejecución necesita Internet y puede descargar varios cientos de MB de fuentes y herramientas. El usuario final no necesita Rust, Python ni Visual Studio.

El resultado es una **carpeta portable**, no un instalador ni un ZIP terminado. Distribuí la carpeta completa con sus modelos, DLL y licencias. El script local no publica releases. El workflow manual de GitHub todavía genera un ZIP básico del ejecutable; no incluye las herramientas de Smooth Motion del paquete completo.

Si ya tenés una compilación release y todas sus herramientas preparadas, `./scripts/package.ps1 -UseExistingBuild` arma la carpeta sin recompilar ni descargar. Una vez verificado el portable, podés eliminar `target/`: contiene datos regenerables. La próxima compilación regenerará las cachés y la preparación de Smooth volverá a descargar sus herramientas si hace falta. El portable de `dist/Recorddy/` funciona independientemente de `target/`.

Antes de una distribución pública falta fijar el hash del ZIP antiguo de RIFE y completar la revisión de licencias y procedencia de los componentes distribuidos. Consultá [FFMPEG-NOTICE.txt](FFMPEG-NOTICE.txt).

## Estructura

| Ruta | Contenido |
| --- | --- |
| `src/app/` | Bandeja, modos del ejecutable, atajos globales y estado de la aplicación. |
| `src/record/` | Captura, audio, codificación, búfer de clips y grabación continua. |
| `src/editor/` | Biblioteca, reproducción, línea de tiempo y exportación. |
| `src/settings/` | Ventana de ajustes. |
| `src/ui_kit/` | Tema, widgets, iconos y comportamiento de las ventanas. |
| `src/i18n.rs` | Idiomas y macros de traducción. |
| `native/rife-stream/` | Fuentes del auxiliar C++ necesarias para reconstruir el portable. |
| `scripts/` | Preparación de herramientas y empaquetado del portable. |
| `docs/exportacion.md` | Arquitectura, límites y validación de Smooth Motion. |
| `.github/` | CI y plantillas de issues y PRs. |

`target/` contiene compilaciones, descargas y cachés locales. `dist/` contiene paquetes generados. Ninguna de estas carpetas se incluye en Git.

Para ejecutar el portable solo necesitás el contenido de `dist/Recorddy/`. `scripts/` y `native/` pertenecen al código fuente: se conservan para volver a generar el paquete, pero no se copian al portable ni se utilizan al abrir la aplicación.

## Modos del ejecutable

```powershell
./target/release/recorddy.exe
./target/release/recorddy.exe --background
./target/release/recorddy.exe --settings
./target/release/recorddy.exe --editor
./target/release/recorddy.exe --editor "C:\Videos\clip.mp4"
```

Sin argumentos abre el proceso de la bandeja y la ventana principal. `--background` inicia en la bandeja sin abrir la ventana; `--settings` abre los ajustes y `--editor` abre el editor, opcionalmente con un archivo.

## Licencia

Recorddy se distribuye bajo **GPL-3.0-or-later**. Consultá [LICENSE](LICENSE), [NOTICE.md](NOTICE.md) y [FFMPEG-NOTICE.txt](FFMPEG-NOTICE.txt). Los componentes externos conservan sus propias licencias; sus avisos deben acompañar al paquete distribuido.
