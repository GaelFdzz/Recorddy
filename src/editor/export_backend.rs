//! Perfiles de codificación de exportación. La disponibilidad se comprueba codificando,
//! no por marca de placa ni por la lista de codecs compilados en FFmpeg.

use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Encoder {
    Nvidia,
    Amd,
    Intel,
    Cpu,
}

impl Encoder {
    pub(super) const ALL: [Self; 4] = [Self::Nvidia, Self::Amd, Self::Intel, Self::Cpu];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Nvidia => "h264_nvenc",
            Self::Amd => "h264_amf",
            Self::Intel => "h264_qsv",
            Self::Cpu => "libx264",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Cpu => "CPU",
        }
    }

    pub(super) fn configure(self, cmd: &mut Command) {
        cmd.args(["-c:v", self.name(), "-profile:v", "high"]);
        // Los controles de calidad no son equivalentes entre fabricantes. Estos son perfiles
        // conservadores; deben compararse visualmente en hardware real de cada familia.
        match self {
            Self::Nvidia => {
                cmd.args(["-preset", "p6", "-tune", "hq", "-rc", "vbr", "-cq", "16", "-b:v", "0"]);
            }
            Self::Amd => {
                cmd.args(["-usage", "transcoding", "-quality", "quality", "-rc", "cqp", "-qp_i", "16", "-qp_p", "18", "-qp_b", "20"]);
            }
            Self::Intel => {
                cmd.args(["-preset", "medium", "-global_quality", "16", "-look_ahead", "0"]);
            }
            Self::Cpu => {
                let threads = std::thread::available_parallelism().map_or(1, |n| n.get() / 2).clamp(1, 4);
                cmd.args(["-preset", "medium", "-crf", "16", "-threads", &threads.to_string()]);
            }
        }
        // NV12 es aceptado por las tres APIs de hardware, incluso QSV sin superficies externas.
        cmd.args(["-pix_fmt", "nv12"]);
    }

    pub(super) fn requested(self) -> bool {
        #[cfg(feature = "devtools")]
        if let Ok(name) = std::env::var("RECORDDY_ENCODER") {
            return name == "auto" || name == self.name();
        }
        true
    }
}
