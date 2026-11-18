//! Escritura incremental para grabaciones largas.
//!
//! No guarda los paquetes de toda la sesión en RAM: cada paquete H.264 se
//! entrega al Sink Writer de Media Foundation conforme llega del encoder.
//! Esta pieza es independiente del grabador de clips y se conectará al hilo
//! de captura en la siguiente integración.

use std::path::PathBuf;

use anyhow::{bail, Result};
use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;

use super::encoder::Packet;
use crate::mf::{self, time_of, Mf, FRAME_BYTES};

/// Un MP4 abierto durante la captura. Debe crearse, utilizarse y finalizarse
/// desde el mismo hilo, pues Media Foundation usa objetos COM.
pub struct ContinuousMux {
    _mf: Mf,
    writer: IMFSinkWriter,
    video_stream: u32,
    audio_stream: Option<u32>,
    path: PathBuf,
    base_pts: Option<i64>,
    last_video_pts: Option<i64>,
    last_audio_frame: i64,
    finished: bool,
}

impl ContinuousMux {
    /// Crea un MP4 de escritura incremental. El archivo solo es reproducible
    /// después de `finish` (el contenedor necesita su índice final).
    pub fn start(path: PathBuf, video_type: &[u8], with_audio: bool) -> Result<Self> {
        let mf = Mf::init();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        unsafe {
            let video = MFCreateMediaType()?;
            MFInitAttributesFromBlob(&video, video_type)?;

            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2)?;
            let attributes = attributes.unwrap();
            attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
            attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;

            let url = HSTRING::from(path.as_os_str());
            let writer = MFCreateSinkWriterFromURL(&url, None, &attributes)?;
            let video_stream = writer.AddStream(&video)?;
            writer.SetInputMediaType(video_stream, &video, None)?;

            let audio_stream = if with_audio {
                let stream = writer.AddStream(&mf::aac_type()?)?;
                writer.SetInputMediaType(stream, &mf::pcm_type()?, None)?;
                Some(stream)
            } else {
                None
            };
            writer.BeginWriting()?;
            Ok(Self {
                _mf: mf,
                writer,
                video_stream,
                audio_stream,
                path,
                base_pts: None,
                last_video_pts: None,
                last_audio_frame: 0,
                finished: false,
            })
        }
    }

    /// El primer paquete debe ser un fotograma clave. Los tiempos del archivo
    /// empiezan en cero aunque el encoder lleve minutos funcionando.
    pub fn write_video(&mut self, packet: &Packet, frame_duration: i64) -> Result<()> {
        if self.base_pts.is_none() {
            if !packet.key {
                bail!("la grabación continua debe empezar con un fotograma clave");
            }
            self.base_pts = Some(packet.pts);
        }
        let base = self.base_pts.unwrap();
        let pts = packet.pts - base;
        if pts < 0 || self.last_video_pts.is_some_and(|prev| pts <= prev) {
            bail!("timestamps de video desordenados en grabación continua");
        }
        unsafe {
            let sample = mf::sample(&packet.data, pts, frame_duration.max(1))?;
            if packet.key {
                sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
            }
            self.writer.WriteSample(self.video_stream, &sample)?;
        }
        self.last_video_pts = Some(pts);
        Ok(())
    }

    /// Acepta muestras PCM estéreo 48 kHz en porciones pequeñas, sin acumular
    /// el audio de la sesión entera. `start_frame` es relativo al inicio del MP4.
    pub fn write_audio(&mut self, pcm: &[i16], start_frame: i64) -> Result<()> {
        let Some(stream) = self.audio_stream else {
            return Ok(());
        };
        if pcm.len() % 2 != 0 || start_frame < self.last_audio_frame {
            bail!("muestras o timestamps de audio inválidos");
        }
        const CHUNK_FRAMES: usize = 1024;
        for (i, chunk) in pcm.chunks(CHUNK_FRAMES * 2).enumerate() {
            let frame = start_frame + (i * CHUNK_FRAMES) as i64;
            let frames = (chunk.len() / 2) as i64;
            let bytes = unsafe {
                std::slice::from_raw_parts(chunk.as_ptr() as *const u8, chunk.len() * std::mem::size_of::<i16>())
            };
            debug_assert_eq!(bytes.len(), frames as usize * FRAME_BYTES);
            let time = time_of(frame);
            let duration = time_of(frame + frames) - time;
            unsafe {
                self.writer.WriteSample(stream, &mf::sample(bytes, time, duration.max(1))?)?;
            }
        }
        self.last_audio_frame = start_frame + (pcm.len() / 2) as i64;
        Ok(())
    }

    /// Finaliza el índice MP4. No debe omitirse al detener una grabación.
    pub fn finish(mut self) -> Result<PathBuf> {
        unsafe { self.writer.Finalize()?; }
        self.finished = true;
        Ok(self.path.clone())
    }
}

impl Drop for ContinuousMux {
    fn drop(&mut self) {
        // Mejor esfuerzo cuando el grabador se cierra inesperadamente.
        // No sustituye el cierre explícito de `finish()`.
        if !self.finished {
            unsafe {
                let _ = self.writer.Finalize();
            }
        }
    }
}
