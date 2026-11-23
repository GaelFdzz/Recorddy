//! Exportación experimental, fuera de la captura. Los originales nunca son destinos de escritura.
//! El auxiliar transmite BGR sin pérdidas; la CLI de PNG queda como respaldo portable.

mod stream;

use std::fs::{self, File};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, MoveFileExW, MOVE_FILE_FLAGS};
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};

use super::export_backend::Encoder;
use super::media::{self, Edit, ExportOpts, Progress};
use crate::i18n::{tr, trf};
use crate::log::log;

const SOCIAL_FPS: u32 = 60;
const DENSE_FPS: u32 = SOCIAL_FPS * 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "u8", into = "u8")]
pub(super) enum Smooth {
    #[default]
    Original,
    Social60,
    Rife120,
}

impl From<u8> for Smooth {
    fn from(value: u8) -> Self {
        // Los modos retirados no activan un experimento distinto sin consentimiento.
        match value {
            5 => Self::Social60,
            3 => Self::Rife120,
            _ => Self::Original,
        }
    }
}

impl From<Smooth> for u8 {
    fn from(value: Smooth) -> Self {
        match value {
            Smooth::Original => 0,
            Smooth::Social60 => 5,
            Smooth::Rife120 => 3,
        }
    }
}

impl Smooth {
    pub(super) fn enabled(self) -> bool {
        self != Self::Original
    }
    pub(super) fn fps(self) -> u32 {
        if self == Self::Rife120 {
            DENSE_FPS
        } else {
            SOCIAL_FPS
        }
    }
}

// Hipótesis de exposición corta, NO reconstrucción de 120 muestras visibles a 60 Hz.
// x es el original remuestreado; en fuentes lentas se reconstruye el cuadro que falta.
// y es el punto medio siguiente. No se mezclan originales vecinos globalmente.
// Se descartan diferencias grandes (oclusiones/cortes), se limita la corrección a 2/255 por canal
// y se protege el detalle con el gradiente espacial. Las zonas estáticas quedan idénticas antes
// de codificar. Los umbrales requieren comparación visual; no representan confianza de RIFE.
const COLOR_FILTER: &str = "scale=in_range=pc:out_color_matrix=bt709:out_range=tv,format=yuv420p";

fn social_filter(reconstruct_base: bool) -> String {
    // lut2 precalcula 256×256 valores por canal, en vez de interpretar una expresión por píxel.
    let delta = "if(lt(abs(x-y),24),x+clip(round((y-x)/8),-2,2),x)";
    let inputs = if reconstruct_base {
        format!("[1:v]split[anchors][midpoints];[anchors]select='not(mod(n,2))',setpts=N/({SOCIAL_FPS}*TB),format=gbrp,split=3[base][detail][edge];[midpoints]select='mod(n,2)',setpts=N/({SOCIAL_FPS}*TB),format=gbrp[mid];")
    } else {
        format!("[0:v]format=gbrp,split=3[base][detail][edge];[1:v]select='mod(n,2)',setpts=N/({SOCIAL_FPS}*TB),format=gbrp[mid];")
    };
    format!(
        "{inputs}\
        [detail][mid]lut2=c0='{delta}':c1='{delta}':c2='{delta}'[exposure];\
        [edge]format=gray,sobel=scale=1,lut=y='clip((24-val)*16,0,255)',format=gbrp[mask];\
        [base][exposure][mask]maskedmerge=planes=7,{COLOR_FILTER}[v]"
    )
}

struct Workspace(PathBuf);

impl Workspace {
    fn new(parent: &Path) -> Result<Self> {
        let parent = std::path::absolute(parent)?;
        fs::create_dir_all(&parent).context(tr!(
            "No se pudo abrir la carpeta de exportación",
            "Couldn't open the export folder",
            "Não foi possível abrir a pasta de exportação"
        ))?;
        let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!("{}{}-{id}", media::EXPORT_WORK_PREFIX, std::process::id()));
        fs::create_dir(&path).context(tr!(
            "No se pudo crear la carpeta temporal",
            "Couldn't create the temporary folder",
            "Não foi possível criar a pasta temporária"
        ))?;
        Ok(Self(path))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.0) {
            log(&format!("Smooth Motion: no se pudieron limpiar {}: {e}", self.0.display()));
        }
    }
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Tools {
    ffmpeg: PathBuf,
    rife: PathBuf,
    model: PathBuf,
    stream: PathBuf,
}

impl Tools {
    fn find() -> Result<Self> {
        let exe = std::env::current_exe()?;
        let dir =
            exe.parent().context(tr!("No se encontró la carpeta de Recorddy", "Recorddy folder not found", "Pasta do Recorddy não encontrada"))?.join("tools");
        let rife = dir.join("rife");
        let model = [rife.join("models/rife-v4.6"), rife.join("rife-v4.6")]
            .into_iter()
            .find(|p| p.join("flownet.param").is_file() && p.join("flownet.bin").is_file())
            .unwrap_or_else(|| rife.join("rife-v4.6"));
        let tools = Self { ffmpeg: dir.join("ffmpeg.exe"), rife: rife.join("rife-ncnn-vulkan.exe"), stream: rife.join("recorddy-rife-stream.exe"), model };
        if !tools.ffmpeg.is_file() {
            bail!(tr!(
                "Falta FFmpeg portátil. Usá el paquete completo de Recorddy.",
                "Portable FFmpeg missing. Use the complete Recorddy package.",
                "FFmpeg portátil ausente. Use o pacote completo do Recorddy."
            ));
        }
        Ok(tools)
    }
    fn require_rife(&self) -> Result<()> {
        if !self.rife.is_file() || !self.model.join("flownet.param").is_file() || !self.model.join("flownet.bin").is_file() {
            bail!(tr!(
                "Falta RIFE portátil o su modelo v4.6. Usá el paquete completo de Recorddy.",
                "Portable RIFE or its v4.6 model is missing. Use the complete Recorddy package.",
                "RIFE portátil ou seu modelo v4.6 ausente. Use o pacote completo do Recorddy."
            ));
        }
        Ok(())
    }
    fn ffmpeg(&self, work: &Path) -> Command {
        let mut cmd = Command::new(&self.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-xerror", "-nostdin", "-y", "-stats_period", "0.2", "-progress"]).arg(work.join("progress.txt"));
        cmd
    }
}

enum Watch<'a> {
    Probe,
    Time(f64),
    Frames(&'a Path, usize),
}

fn png_count(dir: &Path) -> Result<usize> {
    fs::read_dir(dir)?.try_fold(0, |count, entry| Ok(count + usize::from(entry?.path().extension().is_some_and(|x| x == "png"))))
}

/// Todos los hijos son directos, sin shell ni nietos; cancelar o priorizar captura los termina.
fn run(cmd: &mut Command, name: &str, work: &Path, progress: &Progress, range: (f32, f32), watch: Watch<'_>) -> Result<f64> {
    progress.check()?;
    progress.stage(name);
    let begin = Instant::now();
    let stderr = work.join("stderr.txt");
    let _ = fs::remove_file(work.join("progress.txt"));
    let err_file = File::create(&stderr).context(tr!(
        "No se pudo crear el registro de exportación",
        "Couldn't create the export log",
        "Não foi possível criar o registro de exportação"
    ))?;
    let child = cmd
        .creation_flags((CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS).0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(err_file))
        .spawn()
        .with_context(|| trf!("No se pudo iniciar {name}", "Couldn't start {name}", "Não foi possível iniciar {name}"))?;
    let mut child = Process(child);
    log(&format!("Smooth Motion: {name} iniciado"));
    let mut updated = Instant::now();
    loop {
        progress.check()?;
        if matches!(watch, Watch::Probe) && begin.elapsed() > Duration::from_secs(20) {
            bail!(tr!("La comprobación del encoder superó el tiempo límite", "Encoder check timed out", "A verificação do codificador excedeu o tempo limite"));
        }
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                let msg = fs::read_to_string(&stderr).unwrap_or_default();
                let detail: String = msg.chars().rev().take(1200).collect::<String>().chars().rev().collect();
                bail!(trf!("Falló {name}: {detail}", "{name} failed: {detail}", "Falha em {name}: {detail}"));
            }
            break;
        }
        if updated.elapsed() >= Duration::from_millis(500) {
            let fraction = match watch {
                Watch::Probe => 0.0,
                Watch::Frames(dir, count) => png_count(dir)? as f32 / count.max(1) as f32,
                Watch::Time(seconds) => {
                    fs::read_to_string(work.join("progress.txt"))
                        .ok()
                        .and_then(|s| s.lines().rev().find_map(|l| l.strip_prefix("out_time_us=")?.parse::<f32>().ok()))
                        .unwrap_or(0.0)
                        / (seconds as f32 * 1e6).max(1.0)
                }
            };
            progress.set(range.0 + fraction.min(0.99) * range.1);
            updated = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    progress.set(range.0 + range.1);
    let seconds = begin.elapsed().as_secs_f64();
    log(&format!("Smooth Motion: {name} terminado en {seconds:.3} s"));
    Ok(seconds)
}

struct Source {
    path: PathBuf,
    start: f64,
    seconds: f64,
    audio: bool,
    direct: bool,
    range: &'static str,
    sample_rate: u32,
    still: bool,
    streaming: bool,
}

/// Experimental SDR BT.709. Sin metadatos usa el convenio SDR de la captura; rechaza otros
/// espacios conocidos en vez de etiquetarlos silenciosamente como 709.
fn color_range(path: &Path) -> Result<&'static str> {
    unsafe {
        let reader = media::reader(path, false, None, false)?;
        let mt = reader.GetNativeMediaType(media::VIDEO, 0)?;
        for (key, supported) in [
            (MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0),
            (MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0),
            (MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0),
        ] {
            if mt.GetUINT32(&key).is_ok_and(|v| v != 0 && v != supported as u32) {
                bail!(tr!(
                    "Smooth Motion experimental admite SDR BT.709. Exportá este color con Original.",
                    "Experimental Smooth Motion supports SDR BT.709. Export this color with Original.",
                    "O Smooth Motion experimental aceita SDR BT.709. Exporte esta cor com Original."
                ));
            }
        }
        let range = mt.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE).unwrap_or(0);
        if range != 0 && range != MFNominalRange_0_255.0 as u32 && range != MFNominalRange_16_235.0 as u32 {
            bail!(tr!(
                "Rango de color no compatible con Smooth Motion",
                "Color range unsupported by Smooth Motion",
                "Faixa de cor incompatível com Smooth Motion"
            ));
        }
        Ok(if range == MFNominalRange_0_255.0 as u32 { "pc" } else { "tv" })
    }
}

fn source(edit: &Edit, opts: &ExportOpts, work: &Path, p: &Progress) -> Result<Source> {
    p.stage(tr!("Preparando edición", "Preparing edit", "Preparando edição"));
    p.check()?;
    let first = edit.first_video().context(tr!("No hay video para exportar", "No video to export", "Não há vídeo para exportar"))?;
    let info = media::probe(&first.path)?;
    let range = color_range(&first.path)?;
    let mut full_range = range == "pc";
    for c in edit.video.iter().flatten().filter(|c| c.path != first.path) {
        full_range |= color_range(&c.path)? == "pc";
    }
    let seconds = edit.len() as f64 / 1e7;
    let audio = edit.audio.iter().any(|t| !t.is_empty());
    if opts.target.is_none_or(|t| t.height == 0) {
        if let Some(parts @ [clip]) = edit.copy_parts((info.width, info.height)) {
            if !audio || edit.plain_audio(parts) {
                log("Smooth Motion: decodificación directa del recorte; sin H.264 intermedio");
                return Ok(Source {
                    path: clip.path.clone(),
                    start: clip.src_in as f64 / 1e7,
                    seconds,
                    audio,
                    direct: true,
                    range,
                    sample_rate: SOCIAL_FPS,
                    still: false,
                    streaming: false,
                });
            }
        }
    }
    if full_range {
        bail!(tr!(
            "Smooth Motion todavía no admite composiciones con color de rango completo. Usá Original.",
            "Smooth Motion does not yet support full-range color compositions. Use Original.",
            "O Smooth Motion ainda não aceita composições com cor de faixa completa. Use Original."
        ));
    }
    // Las composiciones conservan el render existente. No se promete un intermedio sin pérdida
    // para mezclas, transformaciones o varias piezas.
    let bitrate = (info.width as f64 * info.height as f64 * info.fps() * 0.48).clamp(12e6, 80e6) as u32;
    let stage_opts = ExportOpts {
        bitrate,
        target: opts.target.map(|mut t| {
            t.fps = 0;
            t.bitrate = bitrate;
            t.constant = false;
            t
        }),
    };
    let path = work.join("source.mp4");
    media::export(edit, &path, &stage_opts, p)?;
    Ok(Source { path, start: 0.0, seconds, audio, direct: false, range: "tv", sample_rate: SOCIAL_FPS, still: false, streaming: false })
}

fn check_space(dir: &Path, width: u32, height: u32, seconds: f64) -> Result<()> {
    let mut free = 0;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir), Some(&mut free), None, None) }.context(tr!(
        "No se pudo consultar el espacio libre",
        "Couldn't check free disk space",
        "Não foi possível consultar o espaço livre"
    ))?;
    // Cota conservadora: tres secuencias RGB sin comprimir, PNG y margen para MP4/intermedio.
    let needed = width as f64 * height as f64 * (seconds * SOCIAL_FPS as f64).ceil() * 9.1 + seconds * 20e6 + 64e6;
    if (free as f64) < needed {
        let gb = needed / 1e9;
        bail!(trf!(
            "Smooth Motion necesita hasta {gb:.1} GB libres para temporales. Elegí otra carpeta o un tramo más corto.",
            "Smooth Motion needs up to {gb:.1} GB free for temporary files. Choose another folder or a shorter segment.",
            "O Smooth Motion precisa de até {gb:.1} GB livres para temporários. Escolha outra pasta ou um trecho menor."
        ));
    }
    Ok(())
}

fn destination(path: &Path) -> Result<()> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || ["COM", "LPT"].iter().any(|prefix| base.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9')));
    if name.is_empty() || reserved || name.ends_with([' ', '.']) || name.chars().any(|c| c.is_control() || super::library::BAD_NAME.contains(c)) {
        bail!(tr!("Elegí un nombre de archivo válido para Windows", "Choose a valid Windows file name", "Escolha um nome de arquivo válido para Windows"));
    }
    if path.exists() {
        bail!(tr!("El destino ya existe; elegí otro nombre", "Destination already exists; choose another name", "O destino já existe; escolha outro nome"));
    }
    Ok(())
}

fn publish(tmp: &Path, dest: &Path, p: &Progress) -> Result<()> {
    p.check()?;
    // Mismo volumen y sin REPLACE_EXISTING: tampoco sobrescribe si otro proceso creó el destino.
    unsafe { MoveFileExW(&HSTRING::from(tmp), &HSTRING::from(dest), MOVE_FILE_FLAGS(0)) }.context(tr!(
        "No se pudo publicar el MP4; comprobá el nombre y los permisos",
        "Couldn't publish the MP4; check the name and permissions",
        "Não foi possível publicar o MP4; verifique o nome e as permissões"
    ))?;
    Ok(())
}

#[derive(serde::Serialize)]
struct EncoderTrial {
    encoder: &'static str,
    available: bool,
    seconds: f64,
}

fn encoders(tools: &Tools, work: &Path, size: (u32, u32), fps: u32, p: &Progress) -> Result<(Vec<Encoder>, Vec<EncoderTrial>)> {
    let mut available = Vec::new();
    let mut trials = Vec::new();
    for encoder in Encoder::ALL.into_iter().filter(|e| e.requested()) {
        p.check()?;
        let tmp = work.join("encoder-probe.mp4");
        let begin = Instant::now();
        let mut cmd = tools.ffmpeg(work);
        cmd.args(["-f", "lavfi", "-i", &format!("color=black:s={}x{}:r={fps}", size.0, size.1), "-frames:v", "4", "-an"]);
        encoder.configure(&mut cmd);
        cmd.arg(&tmp);
        let result = run(&mut cmd, tr!("Comprobando codificadores", "Checking encoders", "Verificando codificadores"), work, p, (1.0, 0.0), Watch::Probe)
            .and_then(|_| media::probe(&tmp).map(|_| ()));
        p.check()?;
        let ok = result.is_ok();
        log(&format!(
            "Exportación: prueba {}: {}",
            encoder.name(),
            match result {
                Ok(()) => "disponible".to_string(),
                Err(e) => format!("no disponible ({e:#})"),
            }
        ));
        trials.push(EncoderTrial { encoder: encoder.name(), available: ok, seconds: begin.elapsed().as_secs_f64() });
        if ok {
            available.push(encoder);
        }
        fs::remove_file(&tmp).ok();
    }
    if available.is_empty() {
        bail!(tr!(
            "No hay un codificador utilizable para esta resolución y FPS. Revisá los drivers y el paquete de herramientas.",
            "No usable encoder for this resolution and FPS. Check drivers and the tools package.",
            "Nenhum codificador utilizável para esta resolução e FPS. Verifique os drivers e o pacote de ferramentas."
        ));
    }
    Ok((available, trials))
}

fn encode(tools: &Tools, source: &Source, work: &Path, mode: Smooth, dest: &Path, p: &Progress, encoders: &[Encoder]) -> Result<(f64, &'static str)> {
    let streaming = source.streaming && mode.enabled();
    let expected = media::probe(&source.path)?;
    let fps = mode.fps().to_string();
    let make = |encoder: Encoder| {
        let mut cmd = tools.ffmpeg(work);
        let images = if mode == Smooth::Rife120 { "out" } else { "in" };
        let input_fps = if mode == Smooth::Rife120 { DENSE_FPS } else { source.sample_rate }.to_string();
        let frames = if source.still { work.join("in/00000001.png") } else { work.join(images).join("%08d.png") };
        if streaming {
            cmd.args([
                "-f",
                "rawvideo",
                "-pixel_format",
                "bgr24",
                "-video_size",
                &format!("{}x{}", expected.width, expected.height),
                "-framerate",
                &DENSE_FPS.to_string(),
                "-i",
                "pipe:0",
            ]);
        } else {
            cmd.args(["-f", "image2", "-framerate", &input_fps, "-start_number", "1", "-i"]).arg(frames);
        }
        if mode == Smooth::Social60 && !source.still && !streaming {
            let frames = if source.still { work.join("in/00000001.png") } else { work.join("out/%08d.png") };
            cmd.args(["-f", "image2", "-framerate", &DENSE_FPS.to_string(), "-start_number", "1", "-i"]).arg(frames);
        }
        let audio_index = if mode == Smooth::Social60 && !source.still && !streaming { 2 } else { 1 };
        // El audio se limita al tramo exacto. El video termina en el siguiente límite de cuadro,
        // evitando que -t redondee distinto en la referencia simple y en filter_complex.
        cmd.arg("-ss").arg(format!("{:.7}", source.start)).arg("-t").arg(format!("{:.7}", source.seconds)).arg("-i").arg(&source.path);
        if mode == Smooth::Social60 && !source.still {
            let filter = if streaming { social_filter(true).replace("[1:v]", "[0:v]") } else { social_filter(source.sample_rate < SOCIAL_FPS) };
            cmd.args(["-filter_complex", &filter, "-map", "[v]"]);
        } else {
            let color = if source.sample_rate < SOCIAL_FPS && mode != Smooth::Rife120 {
                format!("fps={SOCIAL_FPS}:start_time=0,{COLOR_FILTER}")
            } else {
                COLOR_FILTER.to_string()
            };
            let filter = if source.still { format!("{color},format=nv12,loop=loop=-1:size=1") } else { color };
            cmd.args(["-map", "0:v:0", "-vf", &filter]);
        }
        if source.audio {
            cmd.arg("-map").arg(format!("{audio_index}:a:0?"));
        }
        encoder.configure(&mut cmd);
        #[cfg(feature = "devtools")]
        if encoder == Encoder::Nvidia && std::env::var_os("RECORDDY_TEST_NVENC_FAILURE").is_some() {
            cmd.args(["-gpu", "999"]);
        }
        if source.direct {
            cmd.args(["-c:a", "aac", "-b:a", "192k"]);
        } else {
            cmd.args(["-c:a", "copy"]);
        }
        cmd.args([
            "-fps_mode",
            "cfr",
            "-r",
            &fps,
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
            "-bsf:v",
            "h264_metadata=colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1:video_full_range_flag=0",
            "-movflags",
            "+faststart",
            "-t",
        ])
        .arg(format!("{:.9}", (source.seconds * mode.fps() as f64).ceil() / mode.fps() as f64))
        .arg(dest);
        cmd
    };
    let begin = Instant::now();
    let mut selected = None;
    let mut error = None;
    for &encoder in encoders {
        let name = encoder.name();
        let device = encoder.label();
        let result = if streaming {
            stream::run(&mut make(encoder), tools, source, work, (expected.width, expected.height), mode, p)
        } else {
            run(
                &mut make(encoder),
                &trf!("Codificando · {device}", "Encoding · {device}", "Codificando · {device}"),
                work,
                p,
                (0.85, 0.14),
                Watch::Time(source.seconds),
            )
        };
        match result {
            Ok(_) => {
                selected = Some(name);
                break;
            }
            Err(e) => {
                p.check()?;
                log(&format!("Smooth Motion: {} falló ({e:#}); se intenta la próxima alternativa comprobada", encoder.name()));
                fs::remove_file(dest).ok();
                error = Some(e);
            }
        }
    }
    let selected = selected.ok_or_else(|| error.unwrap_or_else(|| anyhow::anyhow!(tr!("No hay codificadores", "No encoders", "Nenhum codificador"))))?;
    let probe = media::probe(dest).context(tr!("El MP4 exportado no es válido", "Exported MP4 is invalid", "O MP4 exportado é inválido"))?;
    let tolerance = 1.0 / mode.fps() as f64 + 0.025;
    if probe.width != expected.width
        || probe.height != expected.height
        || (probe.fps() - mode.fps() as f64).abs() > 0.01
        || (probe.duration as f64 / 1e7 - source.seconds).abs() > tolerance
        || (source.audio && expected.audio.is_some() && probe.audio.is_none())
    {
        bail!(tr!(
            "El MP4 no pasó la validación de resolución, FPS, duración o audio",
            "MP4 failed resolution, FPS, duration or audio validation",
            "O MP4 falhou na validação de resolução, FPS, duração ou áudio"
        ));
    }
    Ok((begin.elapsed().as_secs_f64(), selected))
}

pub(super) fn export(edit: &Edit, out: &Path, opts: &ExportOpts, mode: Smooth, p: &Progress) -> Result<String> {
    destination(out)?;
    if !mode.enabled() {
        let work = Workspace::new(out.parent().unwrap_or(Path::new(".")))?;
        let tmp = work.0.join("final.mp4");
        let result = media::export(edit, &tmp, opts, p)?;
        publish(&tmp, out, p)?;
        return Ok(result);
    }
    export_variants(edit, opts, &[(mode, out.to_path_buf())], p)?;
    Ok(match mode {
        Smooth::Social60 => tr!("Smooth Social 60 experimental", "Experimental Smooth Social 60", "Smooth Social 60 experimental"),
        _ => tr!("Smooth 120", "Smooth 120", "Smooth 120"),
    }
    .to_string())
}

/// PNG comparte la inferencia A/B/C; memoria procesa B y C sin guardar sus cuadros.
fn export_variants(edit: &Edit, opts: &ExportOpts, variants: &[(Smooth, PathBuf)], p: &Progress) -> Result<()> {
    let begin = Instant::now();
    p.protect_capture();
    p.check()?;
    for (_, out) in variants {
        destination(out)?;
    }
    let tools = Tools::find()?;
    let work = Workspace::new(variants[0].1.parent().unwrap_or(Path::new(".")))?;
    let info = media::probe(&edit.first_video().context(tr!("No hay video", "No video", "Não há vídeo"))?.path)?;
    let (width, height) = media::canvas_size((info.width, info.height), edit.ratio, opts.target.map_or(0, |t| t.height));
    let seconds = edit.len() as f64 / 1e7;
    if !seconds.is_finite() || seconds <= 0.0 {
        bail!(tr!("El tramo está vacío", "The segment is empty", "O trecho está vazio"));
    }
    p.range(0.0, 0.15);
    let mut source = source(edit, opts, &work.0, p)?;
    // Se conserva la información de fuentes de alta frecuencia: no reducirlas a 60 para
    // volver a inventar sus puntos medios con RIFE. Esto no cambia la captura habitual.
    let source_fps = media::probe(&source.path)?.fps();
    let native_samples = source_fps >= DENSE_FPS as f64;
    source.sample_rate = if native_samples || source_fps < 1.0 { SOCIAL_FPS } else { source_fps.round().clamp(1.0, SOCIAL_FPS as f64) as u32 };
    let (available, encoder_trials) = encoders(&tools, &work.0, (width, height), variants.iter().map(|(mode, _)| mode.fps()).max().unwrap_or(SOCIAL_FPS), p)?;
    let render_seconds = begin.elapsed().as_secs_f64();
    p.range(0.15, 0.85);
    let mut stream_available = tools.stream.is_file();
    #[cfg(feature = "devtools")]
    if std::env::var_os("RECORDDY_SMOOTH_PNG").is_some() {
        stream_available = false;
    }
    if stream_available && !native_samples && variants.len() == 1 {
        source.streaming = true;
        let (mode, dest) = &variants[0];
        let tmp = work.0.join("final-0.mp4");
        let encoded = tools.require_rife().and_then(|_| encode(&tools, &source, &work.0, *mode, &tmp, p, &available));
        match encoded {
            Ok((elapsed, encoder)) => {
                publish(&tmp, dest, p)?;
                p.set(1.0);
                log(&format!("Smooth Motion: ruta en memoria, sin PNG; preparación {render_seconds:.3} s; interpolación y codificación {elapsed:.3} s; encoder {encoder}; total {:.3} s", begin.elapsed().as_secs_f64()));
                #[cfg(feature = "devtools")]
                if std::env::var_os("RECORDDY_EXPORT_REPORT").is_some() {
                    let report = serde_json::json!({"streaming":true, "preparation_seconds":render_seconds, "pipeline_seconds":elapsed, "total_seconds":begin.elapsed().as_secs_f64(), "encoder":encoder, "width":width, "height":height, "duration_seconds":seconds});
                    fs::write(dest.with_extension("json"), serde_json::to_vec_pretty(&report)?)?;
                }
                return Ok(());
            }
            Err(error) => {
                p.check()?;
                log(&format!("Smooth Motion: ruta en memoria no disponible ({error:#}); se usa el respaldo PNG"));
                stream_available = false;
                source.streaming = false;
                fs::remove_file(&tmp).ok();
            }
        }
    }
    check_space(&work.0, width, height, seconds)?;
    let input = work.0.join("in");
    let output = work.0.join("out");
    fs::create_dir(&input)?;
    fs::create_dir(&output)?;
    let rgb = format!("scale=in_color_matrix=bt709:in_range={}:out_range=pc,format=rgb24", source.range);
    let mut extraction = tools.ffmpeg(&work.0);
    // Decodificar el preroll conserva el cuadro que estaba visible al inicio del corte.
    // fps descarta los tiempos negativos; trim limita por timestamp antes de remuestrear.
    extraction.arg("-noaccurate_seek").arg("-ss").arg(format!("{:.7}", source.start)).arg("-i").arg(&source.path);
    if native_samples {
        let count = (seconds * SOCIAL_FPS as f64).ceil() as usize;
        extraction.args(["-filter_complex", &format!("[0:v]trim=end={seconds:.7},fps={DENSE_FPS}:start_time=0:round=up,tpad=stop_mode=clone:stop_duration=0.04,{rgb},split[dense][step];[step]select='not(mod(n,2))',setpts=N/({SOCIAL_FPS}*TB)[anchor]"),
            "-map", "[anchor]", "-an", "-frames:v", &count.to_string(), "-fps_mode", "passthrough", "-start_number", "1", "-compression_level", "1"])
            .arg(input.join("%08d.png"))
            .args(["-map", "[dense]", "-an", "-frames:v", &(count*2).to_string(), "-fps_mode", "passthrough", "-start_number", "1", "-compression_level", "1"])
            .arg(output.join("%08d.png"));
    } else {
        let rate = source.sample_rate;
        let expected = (seconds * rate as f64).ceil() as usize;
        extraction
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-vf",
                &format!("trim=end={seconds:.7},fps={rate}:start_time=0:round=up,tpad=stop_mode=clone:stop_duration={:.9},{rgb}", 1.0 / rate as f64),
                "-frames:v",
                &expected.to_string(),
                "-fps_mode",
                "passthrough",
                "-start_number",
                "1",
                "-compression_level",
                "1",
            ])
            .arg(input.join("%08d.png"));
    }
    let extraction_seconds =
        run(&mut extraction, tr!("Extrayendo muestras", "Extracting samples", "Extraindo amostras"), &work.0, p, (0.0, 0.12), Watch::Time(seconds))?;
    let count = png_count(&input)?;
    if count == 0 || (count as f64 / source.sample_rate as f64 - seconds).abs() > 1.0 / source.sample_rate as f64 + 0.01 {
        bail!(tr!(
            "No se pudieron extraer todos los fotogramas del tramo",
            "Couldn't extract all segment frames",
            "Não foi possível extrair todos os quadros do trecho"
        ));
    }
    let mut jobs = rife_jobs();
    let dense_count = (count * DENSE_FPS as usize + source.sample_rate as usize / 2) / source.sample_rate as usize;
    let static_begin = Instant::now();
    source.still = !native_samples && static_sequence(&input, count, p)?;
    let static_check_seconds = static_begin.elapsed().as_secs_f64();
    log(&format!("Smooth Motion: comprobación estática {static_check_seconds:.3} s; tramo constante: {}", source.still));
    log(&format!(
        "Smooth Motion: {width}×{height}, {seconds:.7} s, {count} muestras a {} FPS, {dense_count} de salida, modelo {}, -j {jobs}",
        source.sample_rate,
        tools.model.display()
    ));
    source.streaming = stream_available && !source.still && !native_samples;
    let mut rife_seconds = if source.still {
        log("Smooth Motion: todas las muestras son idénticas; se repite el cuadro decodificado en memoria sin RIFE");
        p.set(0.85);
        0.0
    } else if native_samples {
        log("Smooth Motion: se aprovechan muestras nativas a 120 FPS; se omite RIFE");
        validate_sequence(&output, dense_count)?;
        p.set(0.85);
        0.0
    } else if source.streaming {
        tools.require_rife()?;
        log("Smooth Motion: interpolación y codificación conectadas por tuberías sin pérdidas");
        0.0
    } else {
        tools.require_rife()?;
        interpolate(&tools, &work.0, dense_count, &mut jobs, p)?
    };
    let mut timings = Vec::new();
    let mut selected_encoders = Vec::new();
    for (i, (mode, _)) in variants.iter().enumerate() {
        let tmp = work.0.join(format!("final-{i}.mp4"));
        let use_stream = source.streaming && mode.enabled();
        let encoded = encode(&tools, &source, &work.0, *mode, &tmp, p, &available);
        let (elapsed, encoder) = match encoded {
            Err(error) if use_stream => {
                p.check()?;
                log(&format!("Smooth Motion: auxiliar en memoria no disponible ({error:#}); se usa la ruta PNG sin cambiar calidad"));
                source.streaming = false;
                rife_seconds += interpolate(&tools, &work.0, dense_count, &mut jobs, p)?;
                encode(&tools, &source, &work.0, *mode, &tmp, p, &available)?
            }
            result => result?,
        };
        timings.push((format!("{mode:?}"), elapsed));
        selected_encoders.push((format!("{mode:?}"), encoder));
    }
    for (i, (_, dest)) in variants.iter().enumerate() {
        publish(&work.0.join(format!("final-{i}.mp4")), dest, p)?;
    }
    p.set(1.0);
    log(&format!("Smooth Motion: preparación {render_seconds:.3} s; extracción {extraction_seconds:.3} s; RIFE {rife_seconds:.3} s; codificación {timings:?}; total {:.3} s", begin.elapsed().as_secs_f64()));
    log(&format!("Smooth Motion: codificadores comprobados {}, seleccionados {selected_encoders:?}", encoder_trials.len()));
    #[cfg(feature = "devtools")]
    if variants.len() > 1 {
        let report = serde_json::json!({ "source": source.path, "start_seconds": source.start,
            "duration_seconds": seconds, "width": width, "height": height, "input_frames": count, "sampling_fps": source.sample_rate, "dense_frames": dense_count,
            "model": if native_samples || source.still { None } else { Some(&tools.model) }, "rife_jobs": if native_samples || source.still { None } else { Some(&jobs) }, "direct_decode": source.direct,
            "preparation_seconds": render_seconds, "extraction_seconds": extraction_seconds,
            "encoder_trials": encoder_trials, "selected_encoders": selected_encoders, "native_samples": native_samples, "static_samples": source.still, "static_check_seconds": static_check_seconds,
            "streaming": source.streaming, "rife_seconds": rife_seconds, "encoding_seconds": timings, "total_seconds": begin.elapsed().as_secs_f64(),
            "outputs": variants.iter().map(|(_, path)| path).collect::<Vec<_>>() });
        fs::write(variants[0].1.with_extension("json"), serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(())
}

fn rife_jobs() -> String {
    #[cfg(feature = "devtools")]
    if let Ok(jobs) = std::env::var("RECORDDY_RIFE_JOBS") {
        if ["1:1:1", "1:2:1", "1:2:2", "2:2:2", "4:2:2"].contains(&jobs.as_str()) {
            return jobs;
        }
    }
    // Dos escritores aceleraron la misma secuencia de referencia sin cambiar los MP4 obtenidos.
    "2:2:2".to_string()
}

/// Igualdad binaria de PNG sin pérdida: conservadora, sin umbrales ni colisiones de hash.
/// Una diferencia termina el análisis; las escenas móviles habituales solo comparan dos archivos.
fn static_sequence(input: &Path, count: usize, p: &Progress) -> Result<bool> {
    p.stage(tr!("Comprobando movimiento", "Checking motion", "Verificando movimento"));
    let first = fs::read(input.join("00000001.png"))?;
    for i in 2..=count {
        p.check()?;
        let file = input.join(format!("{i:08}.png"));
        if fs::metadata(&file)?.len() != first.len() as u64 || fs::read(file)? != first {
            return Ok(false);
        }
    }
    Ok(true)
}

fn interpolate(tools: &Tools, work: &Path, target_count: usize, jobs: &mut String, p: &Progress) -> Result<f64> {
    let begin = Instant::now();
    let output = work.join("out");
    let attempt = |threads: &str| {
        let mut cmd = Command::new(&tools.rife);
        cmd.current_dir(tools.rife.parent().unwrap()).arg("-i").arg(work.join("in")).arg("-o").arg(&output).arg("-m").arg(&tools.model).args([
            "-n",
            &target_count.to_string(),
            "-f",
            "%08d.png",
            "-j",
            threads,
        ]);
        #[cfg(feature = "devtools")]
        if threads != "1:1:1" && std::env::var_os("RECORDDY_TEST_RIFE_FAILURE").is_some() {
            cmd.args(["-g", "999"]);
        }
        let elapsed = run(
            &mut cmd,
            tr!("Interpolando con RIFE", "Interpolating with RIFE", "Interpolando com RIFE"),
            work,
            p,
            (0.12, 0.73),
            Watch::Frames(&output, target_count),
        )?;
        validate_sequence(&output, target_count)?;
        Ok::<_, anyhow::Error>(elapsed)
    };
    if let Err(error) = attempt(jobs) {
        p.check()?;
        if jobs == "1:1:1" {
            return Err(error);
        }
        log(&format!("RIFE: falló con {jobs} ({error:#}); se reintenta una vez con concurrencia mínima, sin reducir calidad"));
        for entry in fs::read_dir(&output)? {
            p.check()?;
            let entry = entry?;
            if entry.path().extension().is_some_and(|x| x == "png") {
                fs::remove_file(entry.path())?;
            }
        }
        *jobs = "1:1:1".to_string();
        attempt(jobs).context(tr!(
            "No se pudo interpolar con Vulkan. Probá actualizar los drivers o exportá con Original.",
            "Vulkan interpolation failed. Try updating drivers or export with Original.",
            "A interpolação Vulkan falhou. Tente atualizar os drivers ou exporte com Original."
        ))?;
    }
    Ok(begin.elapsed().as_secs_f64())
}

fn validate_sequence(dir: &Path, count: usize) -> Result<()> {
    if png_count(dir)? != count || !(1..=count).all(|i| dir.join(format!("{i:08}.png")).is_file()) {
        bail!(tr!("La secuencia de muestras está incompleta", "The sample sequence is incomplete", "A sequência de amostras está incompleta"));
    }
    Ok(())
}

#[cfg(feature = "devtools")]
pub(super) fn compare(edit: &Edit, out: &Path, opts: &ExportOpts, p: &Progress) -> Result<()> {
    let variants = [
        (Smooth::Original, out.with_extension("A-original-60.mp4")),
        (Smooth::Rife120, out.with_extension("B-reference-120.mp4")),
        (Smooth::Social60, out.with_extension("C-social-60.mp4")),
    ];
    export_variants(edit, opts, &variants, p)
}

#[cfg(all(test, feature = "devtools"))]
mod tests {
    use super::*;

    #[test]
    fn preferencias_anteriores_conservan_el_resto_de_la_edicion() {
        for (id, expected) in [
            (0, Smooth::Original),
            (1, Smooth::Original),
            (2, Smooth::Original),
            (3, Smooth::Rife120),
            (4, Smooth::Original),
            (5, Smooth::Social60),
            (255, Smooth::Original),
        ] {
            let json = format!(r#"{{"export_smooth":{id},"export_height":720,"export_size_mb":50,"magnet":false}}"#);
            let prefs: super::super::prefs::Prefs = serde_json::from_str(&json).unwrap();
            assert_eq!(prefs.export_smooth, expected);
            assert_eq!(prefs.export_height, 720);
            assert_eq!(prefs.export_size_mb, 50);
            assert!(!prefs.magnet);
        }
    }

    #[test]
    fn publicar_no_reemplaza_un_destino_creado_durante_la_exportacion() {
        let dir = Workspace::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let original = dir.0.join("original.mp4");
        let tmp = dir.0.join("final.mp4");
        fs::write(&original, b"original intacto").unwrap();
        fs::write(&tmp, b"nuevo MP4").unwrap();
        assert!(publish(&tmp, &original, &Progress::new()).is_err());
        assert_eq!(fs::read(&original).unwrap(), b"original intacto");
        assert_eq!(fs::read(&tmp).unwrap(), b"nuevo MP4");
    }

    #[test]
    fn cancelar_impide_publicar_y_limpia_el_temporal() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        let dir = Workspace::new(&root).unwrap();
        let path = dir.0.clone();
        let tmp = path.join("final.mp4");
        let out = path.join("exportado.mp4");
        fs::write(&tmp, b"parcial").unwrap();
        let (folders, _) = super::super::library::scan(&root, None, &[]);
        assert!(!folders.iter().any(|folder| folder.path == path), "la biblioteca mostró una exportación incompleta");
        let progress = Progress::new();
        progress.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(publish(&tmp, &out, &progress).is_err());
        assert!(!out.exists());
        drop(dir);
        assert!(!path.exists());
    }
}
