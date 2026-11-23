//! Diagnóstico barato de continuidad temporal de paquetes H.264.
//! Solo observa PTS: NO mide imágenes duplicadas ni fluidez visual del contenido.

use super::encoder::Packet;

#[derive(Default)]
pub struct Cadence {
    first: Option<i64>,
    previous: Option<i64>,
    frames: u64,
    gaps: u64,
    missing_slots: u64,
    repeated_or_reordered_pts: u64,
    worst_gap: i64,
}

impl Cadence {
    pub fn observe(&mut self, pts: i64, expected: i64) {
        self.frames += 1;
        if self.first.is_none() { self.first = Some(pts); }
        if let Some(prev) = self.previous {
            let delta = pts - prev;
            if delta <= 0 {
                self.repeated_or_reordered_pts += 1;
            } else {
                self.worst_gap = self.worst_gap.max(delta);
                // Redondeamos a slots nominales (los PTS usan unidades de 100 ns).
                let slots = ((delta + expected / 2) / expected).max(1) as u64;
                if slots > 1 {
                    self.gaps += 1;
                    self.missing_slots += slots - 1;
                }
            }
        }
        self.previous = Some(pts);
    }

    pub fn from_packets<'a>(packets: impl IntoIterator<Item = &'a Packet>, expected: i64) -> Self {
        let mut result = Self::default();
        for packet in packets { result.observe(packet.pts, expected); }
        result
    }

    pub fn report(&self, expected: i64) -> String {
        let duration = match (self.first, self.previous) {
            (Some(first), Some(last)) => (last - first).max(0),
            _ => 0,
        };
        let elapsed = duration as f64 / 10_000_000.0;
        let effective_fps = if elapsed > 0.0 { (self.frames.saturating_sub(1)) as f64 / elapsed } else { 0.0 };
        let nominal_fps = 10_000_000.0 / expected as f64;
        let planned = self.frames + self.missing_slots;
        let missing_pct = if planned > 0 { self.missing_slots as f64 * 100.0 / planned as f64 } else { 0.0 };
        format!(
            "{} cuadros, {:.2} s, FPS nominal {:.2}, FPS efectivo {:.2}, huecos {}, slots omitidos {} ({:.2}%), PTS repetidos/desordenados {}, mayor intervalo {:.2} ms",
            self.frames, elapsed, nominal_fps, effective_fps, self.gaps, self.missing_slots,
            missing_pct, self.repeated_or_reordered_pts, self.worst_gap as f64 / 10_000.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_missing_slots() {
        let mut c = Cadence::default();
        for pts in [0, 166_667, 500_001, 666_668] { c.observe(pts, 166_667); }
        assert_eq!(c.frames, 4);
        assert_eq!(c.gaps, 1);
        assert_eq!(c.missing_slots, 1);
    }

    #[test]
    fn counts_repeated_pts() {
        let mut c = Cadence::default();
        for pts in [0, 0, 166_667] { c.observe(pts, 166_667); }
        assert_eq!(c.repeated_or_reordered_pts, 1);
    }
}
