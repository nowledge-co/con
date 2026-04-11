use std::time::{Duration, Instant};

use gpui::{Window, px};

#[derive(Clone, Debug)]
pub struct MotionValue {
    current: f32,
    from: f32,
    target: f32,
    started_at: Option<Instant>,
    duration: Duration,
}

impl MotionValue {
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            from: value,
            target: value,
            started_at: None,
            duration: Duration::from_millis(180),
        }
    }

    pub fn is_animating(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn set_target(&mut self, target: f32, duration: Duration) {
        let current = self.current();
        self.current = current;
        self.from = current;
        self.target = target;
        self.duration = duration;
        self.started_at = if (current - target).abs() > 0.001 {
            Some(Instant::now())
        } else {
            self.current = target;
            None
        };
    }

    pub fn restart(&mut self, from: f32, target: f32, duration: Duration) {
        self.current = from;
        self.from = from;
        self.target = target;
        self.duration = duration;
        self.started_at = if (from - target).abs() > 0.001 {
            Some(Instant::now())
        } else {
            None
        };
    }

    pub fn current(&self) -> f32 {
        let Some(started_at) = self.started_at else {
            return self.target;
        };

        let elapsed = started_at.elapsed();
        if elapsed >= self.duration || self.duration.is_zero() {
            return self.target;
        }

        let t = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        let eased = ease_out_quint(t.clamp(0.0, 1.0));
        self.from + ((self.target - self.from) * eased)
    }

    pub fn value(&mut self, window: &mut Window) -> f32 {
        let value = self.current();
        if let Some(started_at) = self.started_at {
            if started_at.elapsed() >= self.duration || self.duration.is_zero() {
                self.current = self.target;
                self.from = self.target;
                self.started_at = None;
            } else {
                self.current = value;
                window.request_animation_frame();
            }
        } else {
            self.current = self.target;
        }
        value
    }
}

pub fn vertical_reveal_offset(progress: f32, distance: f32) -> gpui::Pixels {
    px((1.0 - progress).clamp(0.0, 1.0) * distance)
}

pub fn horizontal_reveal_offset(progress: f32, distance: f32) -> gpui::Pixels {
    px((1.0 - progress).clamp(0.0, 1.0) * distance)
}

fn ease_out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}
