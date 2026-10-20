use std::time::{Duration, Instant};
use pir_protocol::events::{InputEvent, EventMetadata};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub timestamp: Instant,
    pub pointer_id: u32,
}

/// Turns a stream of touch samples (often 120–240 Hz) into relative mouse moves sent at
/// no more than `target_hz`, accumulating the movement in between.
pub struct TouchCoalescer {
    gesture_classifier: GestureClassifier,
    acceleration: AccelerationCurve,
    tracker: Option<TrackedPointer>,
    /// Sub-pixel movement not yet emitted, so slow drags still move the cursor.
    remainder: (f32, f32),
    pending: (i32, i32),
    last_emit: Instant,
    target_interval: Duration,
    sequence_number: u64,
    session_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
struct TrackedPointer {
    pointer_id: u32,
    x: f32,
    y: f32,
    timestamp: Instant,
}

impl TouchCoalescer {
    pub const MIN_HZ: u32 = 1;
    pub const MAX_HZ: u32 = 1000;

    pub fn new(session_id: Uuid, target_hz: u32) -> Self {
        let hz = target_hz.clamp(Self::MIN_HZ, Self::MAX_HZ);
        Self {
            gesture_classifier: GestureClassifier,
            acceleration: AccelerationCurve::default(),
            tracker: None,
            remainder: (0.0, 0.0),
            pending: (0, 0),
            last_emit: Instant::now(),
            target_interval: Duration::from_secs_f64(1.0 / hz as f64),
            sequence_number: 0,
            session_id,
        }
    }

    /// Feed the current set of touches. Pass an empty slice when all fingers lift.
    pub fn process_touches(&mut self, touches: &[TouchPoint]) -> Option<InputEvent> {
        if !matches!(self.gesture_classifier.classify(touches), GestureType::Pan) {
            // Lifting or switching to a multi-finger gesture ends the pan; the next pan
            // starts from wherever that finger lands, not from the old position.
            self.tracker = None;
            return None;
        }

        let point = touches[0];
        match self.tracker {
            Some(prev) if prev.pointer_id == point.pointer_id => self.accumulate(prev, point),
            _ => {}
        }
        self.tracker = Some(TrackedPointer {
            pointer_id: point.pointer_id,
            x: point.x,
            y: point.y,
            timestamp: point.timestamp,
        });

        if self.last_emit.elapsed() >= self.target_interval {
            self.emit_pending()
        } else {
            None
        }
    }

    fn accumulate(&mut self, prev: TrackedPointer, point: TouchPoint) {
        let (dx, dy) = (point.x - prev.x, point.y - prev.y);
        let dt = point.timestamp.saturating_duration_since(prev.timestamp).as_secs_f32();
        let speed = if dt > 0.0 { (dx * dx + dy * dy).sqrt() / dt } else { 0.0 };
        let gain = self.acceleration.gain(speed);

        let x = dx * gain + self.remainder.0;
        let y = dy * gain + self.remainder.1;
        let (whole_x, whole_y) = (x.trunc(), y.trunc());
        self.remainder = (x - whole_x, y - whole_y);
        self.pending.0 = self.pending.0.saturating_add(whole_x as i32);
        self.pending.1 = self.pending.1.saturating_add(whole_y as i32);
    }

    fn emit_pending(&mut self) -> Option<InputEvent> {
        self.last_emit = Instant::now();
        if self.pending == (0, 0) {
            return None;
        }
        let (delta_x, delta_y) = std::mem::take(&mut self.pending);
        self.sequence_number += 1;
        Some(InputEvent::MouseMove {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            delta_x,
            delta_y,
        })
    }

    /// Emits any movement still buffered, e.g. when the finger lifts.
    pub fn flush(&mut self) -> Option<InputEvent> {
        self.emit_pending()
    }

    /// Forgets the current touch and any movement not yet emitted, e.g. when what looked
    /// like a pan turns out to be a tap.
    pub fn discard(&mut self) {
        self.tracker = None;
        self.remainder = (0.0, 0.0);
        self.pending = (0, 0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GestureType { None, Pan, Pinch, Scroll }

struct GestureClassifier;

impl GestureClassifier {
    fn classify(&self, touches: &[TouchPoint]) -> GestureType {
        match touches.len() {
            1 => GestureType::Pan,
            2 => {
                if self.is_pinch(touches) { GestureType::Pinch } else { GestureType::Scroll }
            }
            _ => GestureType::None,
        }
    }

    fn is_pinch(&self, touches: &[TouchPoint]) -> bool {
        if touches.len() != 2 { return false; }
        let d = ((touches[0].x - touches[1].x).powi(2) + (touches[0].y - touches[1].y).powi(2)).sqrt();
        d > 50.0
    }
}

/// Pointer acceleration: 1:1 below `threshold` (px/s), rising linearly above it, capped.
#[derive(Debug, Clone, Copy)]
struct AccelerationCurve {
    threshold: f32,
    acceleration: f32,
    max_gain: f32,
}

impl Default for AccelerationCurve {
    fn default() -> Self {
        Self { threshold: 100.0, acceleration: 1.5, max_gain: 4.0 }
    }
}

impl AccelerationCurve {
    fn gain(&self, speed: f32) -> f32 {
        if speed > self.threshold {
            (1.0 + (speed - self.threshold) / self.threshold * self.acceleration).min(self.max_gain)
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(x: f32, y: f32, at: Instant, pointer_id: u32) -> TouchPoint {
        TouchPoint { x, y, pressure: 1.0, timestamp: at, pointer_id }
    }

    /// Drags in `steps` samples of (`dx`,`dy`) each, `step_ms` apart, then flushes.
    fn drag(c: &mut TouchCoalescer, start: Instant, steps: u32, dx: f32, dy: f32, step_ms: u64) -> (i32, i32) {
        let mut total = (0, 0);
        for i in 0..=steps {
            let at = start + Duration::from_millis(step_ms * i as u64);
            let event = c.process_touches(&[touch(dx * i as f32, dy * i as f32, at, 0)]);
            add(&mut total, event);
        }
        add(&mut total, c.flush());
        total
    }

    fn add(total: &mut (i32, i32), event: Option<InputEvent>) {
        if let Some(InputEvent::MouseMove { delta_x, delta_y, .. }) = event {
            total.0 += delta_x;
            total.1 += delta_y;
        }
    }

    #[test]
    fn slow_drag_moves_by_finger_distance() {
        let mut c = TouchCoalescer::new(Uuid::nil(), 1000);
        // 1 px per 20 ms = 50 px/s, below the acceleration threshold.
        assert_eq!(drag(&mut c, Instant::now(), 40, 1.0, -0.5, 20), (40, -20));
    }

    #[test]
    fn sub_pixel_moves_are_not_lost() {
        let mut c = TouchCoalescer::new(Uuid::nil(), 1000);
        assert_eq!(drag(&mut c, Instant::now(), 100, 0.3, 0.0, 20), (30, 0));
    }

    #[test]
    fn fast_drag_is_accelerated_but_capped() {
        let mut c = TouchCoalescer::new(Uuid::nil(), 1000);
        // 10 px per 4 ms = 2500 px/s.
        let (x, _) = drag(&mut c, Instant::now(), 10, 10.0, 0.0, 4);
        assert!(x > 100 && x <= 400, "got {x}");
    }

    #[test]
    fn new_touch_does_not_jump_from_old_position() {
        let mut c = TouchCoalescer::new(Uuid::nil(), 1000);
        let start = Instant::now();
        drag(&mut c, start, 10, 1.0, 0.0, 20);
        c.process_touches(&[]);

        let later = start + Duration::from_secs(1);
        let mut total = (0, 0);
        add(&mut total, c.process_touches(&[touch(500.0, 500.0, later, 1)]));
        add(&mut total, c.flush());
        assert_eq!(total, (0, 0));
    }

    #[test]
    fn coalesces_to_target_rate() {
        let mut c = TouchCoalescer::new(Uuid::nil(), 60);
        let start = Instant::now();
        let emitted = (0..=20)
            .filter_map(|i| c.process_touches(&[touch(i as f32, 0.0, start + Duration::from_millis(i), 0)]))
            .count();
        // 21 samples fed back-to-back fall inside one 16 ms window.
        assert!(emitted <= 1, "emitted {emitted}");
        assert!(c.flush().is_some());
    }

    #[test]
    fn zero_hz_does_not_panic() {
        TouchCoalescer::new(Uuid::nil(), 0);
    }
}
