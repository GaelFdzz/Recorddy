# Variante local sin actualizaciones remotas

Esta copia del código fuente se preparó a partir del ZIP `clippy-main.zip` facilitado por el usuario.

## Cambios aplicados
- `src/app/update.rs`: reemplazado por una implementación **sin red**. Nunca consulta releases, descarga archivos ni ejecuta instaladores de actualizaciones.
- `src/app/tray.rs`: se eliminó la programación del hilo de comprobación de actualizaciones.
- `src/settings/pages.rs`: se quitó el botón de actualización y se muestra un indicador de versión local.
- `Cargo.toml`: se eliminó la característica `Win32_Networking_WinHttp` que era usada por el actualizador.

## Advertencias importantes
- Este ZIP contiene **código fuente**, NO un ejecutable de Windows validado.
- Los métodos antiguos de comprobación/instalación aún pueden estar referenciados internamente, pero todas sus implementaciones fueron convertidas en funciones sin red que rechazan la operación.
- No se realizó compilación ni prueba de integración sobre Windows en este entorno.
- La versión todavía usa el sistema original de clips temporales; **NO implementa aún grabación continua a disco**.
- No se auditó la cadena completa de suministro de dependencias ni se garantiza ausencia de malware.
- Un fork de GitHub no crea una copia local automáticamente ni impide por sí solo la descarga de cambios al actualizar dependencias; revisar `Cargo.lock` antes de compilar.

## Próxima función: grabación larga
La arquitectura actual mantiene paquetes H.264 y audio PCM en RAM (`record/recorder.rs`) y los convierte en MP4 cuando se guarda (`record/mux.rs`). Para horas de video se necesita un escritor MP4 persistente que abra al iniciar, escriba video/audio por tramos, cierre al detener y maneje fallos, reinicios de encoder, cambios de monitor y cortes de energía. No basta con subir `MAX_SECONDS`.
