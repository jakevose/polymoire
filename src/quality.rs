//! Adaptive render resolution: the scene renders offscreen at a fraction of the window size
//! and is scaled up, so slow GPUs keep a smooth frame rate.

#[derive(Clone, Debug)]
pub struct Quality {
    scale: f32,
    acc: f32,
    frames: u32,
    /// Target frame time in seconds.
    pub target: f32,
}

pub const MIN_SCALE: f32 = 0.35;
pub const MAX_SCALE: f32 = 1.0;

impl Default for Quality {
    fn default() -> Self {
        Quality {
            scale: 0.75,
            acc: 0.0,
            frames: 0,
            target: 1.0 / 60.0,
        }
    }
}

impl Quality {
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Feed one frame's duration. Every 45 frames the scale steps down if frames are slow
    /// and creeps back up if there is headroom.
    pub fn frame(&mut self, dt: f32) {
        self.acc += dt.min(0.25);
        self.frames += 1;
        if self.frames >= 45 {
            let avg = self.acc / self.frames as f32;
            if avg > self.target * 1.8 {
                self.scale = (self.scale * 0.8).max(MIN_SCALE);
            } else if avg < self.target * 1.1 {
                self.scale = (self.scale * 1.08).min(MAX_SCALE);
            }
            self.acc = 0.0;
            self.frames = 0;
        }
    }

    /// Offscreen size for a window of `w` x `h` physical pixels, capped so very large or
    /// high-DPI displays don't render more pixels than the effect needs.
    pub fn render_size(&self, w: f32, h: f32) -> [u32; 2] {
        const MAX_PIXELS: f32 = 2_560.0 * 1_440.0;
        let mut s = self.scale;
        let px = w * h * s * s;
        if px > MAX_PIXELS {
            s *= (MAX_PIXELS / px).sqrt();
        }
        [
            ((w * s).round() as u32).max(1),
            ((h * s).round() as u32).max(1),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_frames_lower_resolution_until_the_floor() {
        let mut q = Quality::default();
        for _ in 0..45 * 30 {
            q.frame(0.1);
        }
        assert_eq!(q.scale(), MIN_SCALE);
    }

    #[test]
    fn fast_frames_raise_resolution_to_full() {
        let mut q = Quality::default();
        for _ in 0..45 * 30 {
            q.frame(0.008);
        }
        assert_eq!(q.scale(), MAX_SCALE);
    }

    #[test]
    fn render_size_is_capped_on_huge_displays() {
        let mut q = Quality::default();
        for _ in 0..45 * 30 {
            q.frame(0.008);
        }
        let [w, h] = q.render_size(7_680.0, 4_320.0);
        assert!((w as f32 * h as f32) <= 2_560.0 * 1_440.0 * 1.01);
        assert_eq!(q.render_size(0.0, 0.0), [1, 1]);
    }
}
