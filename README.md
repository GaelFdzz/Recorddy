# Recorddy

Grabadora de clips y grabación continua para Windows, basada en el proyecto libre [Clippy](https://github.com/santiremedi/clippy) de santiremedi.

**Recorddy es una versión modificada, independiente y no oficial. No está afiliada al autor de Clippy.** Cambia el nombre, los recursos visuales y añade funciones de grabación continua y selección de pantalla. No incluye actualización remota automática en la versión de trabajo.

## Compilar

En Windows con Rust estable, MSVC y Windows SDK:

```powershell
cargo check
cargo build --release
.\target\release\recorddy.exe
```

## Datos de usuario

- Configuración y registros nuevos: `%APPDATA%\Recorddy`.
- La primera vez se copia `config.json` desde `%APPDATA%\Clippy`, si existe y aún no hay configuración de Recorddy. No se elimina nada de la ubicación anterior.
- Los videos existentes NO se mueven; el directorio de salida guardado en la configuración copiada continúa utilizándose hasta que lo cambies manualmente.
- Si Clippy tenía inicio automático con Windows, desactívalo desde la versión anterior o desde las opciones de inicio de Windows; Recorddy utiliza una entrada separada.
- Cierra Clippy antes de iniciar Recorddy para evitar que ambas aplicaciones intenten capturar los mismos recursos.

## Licencia y atribución

Este trabajo deriva de **Clippy**, © sus autores/contribuyentes originales. El código se distribuye bajo **GPL-3.0-or-later**; consulta [LICENSE](LICENSE) para el texto íntegro y [NOTICE.md](NOTICE.md) para los avisos sobre modificaciones. Si redistribuyes binarios, acompáñalos del código fuente correspondiente y los avisos/licencia exigibles.

El nombre y la identidad gráfica de Clippy no se usan para identificar esta versión modificada. Las referencias históricas a Clippy se conservan donde son necesarias para reconocer archivos y configuraciones anteriores y dar atribución.
