//! Grabación: captura de pantalla y audio, codificación en la GPU, buffer en RAM y guardado de clips.

pub mod audio;
mod capture;
mod cadence;
pub mod clock;
mod convert;
mod continuous;
mod encoder;
mod game;
mod mux;
mod screen;
pub mod recorder;

pub use encoder::{Device, CPU_MAX_FPS, CPU_MAX_SHORT};

/// Monitores disponibles para selección en Ajustes.
pub fn monitor_names() -> Vec<String> { capture::Capture::screens() }

/// Opciones para el selector: (identificador estable, nombre visible).
/// Se conserva DISPLAYn internamente, sin cambiar la configuración existente.
pub fn monitor_choices() -> Vec<(String, String)> {
    let friendly = screen::friendly_names();
    capture::Capture::screens().into_iter().map(|id| {
        let model = friendly.iter().find(|(device, _)| device == &id)
            .map(|(_, label)| label.trim()).unwrap_or("");
        // Mostrar el identificador como contexto para monitores idénticos.
        let label = if model.is_empty() {
            format!("Monitor ({id})")
        } else {
            format!("{model} ({id})")
        };
        (id, label)
    }).collect()
}
