// Dibujo de íconos sin dependencias (lo usa también build.rs para el ícono del .exe).
#![allow(dead_code)]

/// Ícono de bandeja de Recorddy, basado en su marca R propia.
/// Color intenso mientras está activo, desaturado cuando está pausado.
pub fn status_rgba(size: u32, on: bool) -> Vec<u8> {
    let mut pixels = app_rgba(size);
    if !on {
        for px in pixels.chunks_exact_mut(4) {
            let gray = ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
            px[0] = gray;
            px[1] = gray;
            px[2] = gray;
        }
    }
    pixels
}

/// Ícono original de Recorddy: fondo grafito, figura R geométrica azul claro.
/// Es distinto del símbolo circular utilizado por el proyecto original.
pub fn app_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let xx = (x as f32 + 0.5) / s;
            let yy = (y as f32 + 0.5) / s;
            let margin = 0.05;
            let corner = 0.16;
            let dx = (xx - 0.5).abs() - (0.5 - margin - corner);
            let dy = (yy - 0.5).abs() - (0.5 - margin - corner);
            let rr = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt()
                + dx.max(dy).min(0.0) - corner;
            let cover = (0.01 - rr) * s;
            let a = cover.clamp(0.0, 1.0);
            // R: asta, barra superior, curva derecha y diagonal inferior.
            let stem = xx >= 0.27 && xx <= 0.36 && yy >= 0.22 && yy <= 0.78;
            let upper = yy >= 0.22 && yy <= 0.31 && xx >= 0.30 && xx <= 0.62;
            let bowl = xx >= 0.56 && xx <= 0.66 && yy >= 0.28 && yy <= 0.49;
            let middle = yy >= 0.45 && yy <= 0.54 && xx >= 0.32 && xx <= 0.61;
            let diagonal = yy >= 0.49 && yy <= 0.78 &&
                (xx - (0.43 + (yy - 0.49) * 0.82)).abs() < 0.053;
            let mark = stem || upper || bowl || middle || diagonal;
            let color = if mark { [105, 193, 232] } else { [17, 22, 30] };
            put(&mut pixels, size, x, y, color, a);
        }
    }
    pixels
}

fn dist(x: u32, y: u32, c: f32) -> f32 {
    let dx = x as f32 + 0.5 - c;
    let dy = y as f32 + 0.5 - c;
    (dx * dx + dy * dy).sqrt()
}

fn put(v: &mut [u8], size: u32, x: u32, y: u32, rgb: [u8; 3], a: f32) {
    let i = ((y * size + x) * 4) as usize;
    v[i..i + 3].copy_from_slice(&rgb);
    v[i + 3] = (a * 255.0).round() as u8;
}
