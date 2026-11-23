//! Tres hijos directos y tuberías acotadas; nunca se guarda la secuencia interpolada.
use super::*;

pub(super) fn run(encoder: &mut Command, tools: &Tools, source: &Source, work: &Path, size: (u32, u32), mode: Smooth, p: &Progress) -> Result<f64> {
    p.check()?;
    p.stage(tr!("Interpolando y codificando", "Interpolating and encoding", "Interpolando e codificando"));
    let begin = Instant::now();
    let count = (source.seconds * source.sample_rate as f64).ceil() as usize;
    let dense = (count * DENSE_FPS as usize + source.sample_rate as usize / 2) / source.sample_rate as usize;
    let rate = source.sample_rate;
    let emit = ((source.seconds * mode.fps() as f64).ceil() as usize * (DENSE_FPS / mode.fps()) as usize).min(dense);
    let mut decoder = Command::new(&tools.ffmpeg);
    decoder.args(["-hide_banner", "-loglevel", "error", "-xerror", "-nostdin", "-noaccurate_seek", "-ss"])
        .arg(format!("{:.7}", source.start)).arg("-i").arg(&source.path)
        .args(["-map", "0:v:0", "-an", "-vf", &format!("trim=end={:.7},fps={rate}:start_time=0:round=up,tpad=stop_mode=clone:stop_duration={:.9},scale=in_color_matrix=bt709:in_range={}:out_range=pc,format=bgr24", source.seconds, 1.0 / rate as f64, source.range), "-frames:v", &count.to_string(), "-fps_mode", "passthrough", "-f", "rawvideo", "pipe:1"]);
    let mut inference = Command::new(&tools.stream);
    inference.arg(&tools.model).args([size.0.to_string(), size.1.to_string(), count.to_string(), dense.to_string(), "2".into(), emit.to_string()]);
    #[cfg(feature = "devtools")]
    if std::env::var_os("RECORDDY_TEST_STREAM_FAILURE").is_some() {
        inference.arg("invalid");
    }
    let _ = fs::remove_file(work.join("progress.txt"));
    let mut children = Vec::new();
    let mut pipe = None;
    for (index, cmd) in [&mut decoder, &mut inference, encoder].into_iter().enumerate() {
        let child = cmd
            .creation_flags((CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS).0)
            .stdin(pipe.take().map_or_else(Stdio::null, Stdio::from))
            .stdout(if index < 2 { Stdio::piped() } else { Stdio::null() })
            .stderr(File::create(work.join(format!("stream-{index}.log")))?)
            .spawn()
            .context(tr!(
                "No se pudo iniciar la exportación en memoria",
                "Couldn't start in-memory export",
                "Não foi possível iniciar a exportação em memória"
            ))?;
        let mut child = Process(child);
        pipe = child.0.stdout.take();
        children.push(child);
    }
    loop {
        p.check()?;
        let mut finished = 0;
        for (index, child) in children.iter_mut().enumerate() {
            if let Some(status) = child.0.try_wait()? {
                if !status.success() {
                    let detail = fs::read_to_string(work.join(format!("stream-{index}.log"))).unwrap_or_default();
                    let detail: String = detail.chars().rev().take(1200).collect::<String>().chars().rev().collect();
                    bail!(trf!("Falló la exportación en memoria: {detail}", "In-memory export failed: {detail}", "Falha na exportação em memória: {detail}"));
                }
                finished += 1;
            }
        }
        if finished == children.len() {
            break;
        }
        let fraction = fs::read_to_string(work.join("progress.txt"))
            .ok()
            .and_then(|s| s.lines().rev().find_map(|l| l.strip_prefix("out_time_us=")?.parse::<f32>().ok()))
            .unwrap_or(0.0)
            / (source.seconds as f32 * 1e6).max(1.0);
        p.set(0.12 + fraction.min(0.99) * 0.87);
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(begin.elapsed().as_secs_f64())
}
