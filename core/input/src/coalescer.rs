use std::collections::VecDeque;
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

#[derive(Debug, Clone, Copy)]
pub struct MouseMoveEvent {
    pub delta_x: i32,
    pub delta_y: i32,
}

pub struct TouchCoalescer {
    gesture_classifier: GestureClassifier,
    velocity_estimator: VelocityEstimator,
    acceleration: AccelerationCurve,
    movement_buffer: VecDeque<MouseMoveEvent>,
    last_emit: Instant,
    target_interval: Duration,
    sequence_number: u64,
    session_id: Uuid,
}

impl TouchCoalescer {
    pub fn new(session_id: Uuid, target_hz: u32) -> Self {
        let target_interval = Duration::from_millis(1000 / target_hz as u64);
        Self {
            gesture_classifier: GestureClassifier::new(),
            velocity_estimator: VelocityEstimator::new(),
            acceleration: AccelerationCurve::default(),
            movement_buffer: VecDeque::with_capacity(64),
            last_emit: Instant::now(),
            target_interval,
            sequence_number: 0,
            session_id,
        }
    }

    pub fn process_touches(&mut self, touches: &[TouchPoint]) -> Option<InputEvent> {
        if touches.is_empty() {
            return None;
        }

        let gesture = self.gesture_classifier.classify(touches);
        
        if !matches!(gesture, GestureType::Pan) {
            return None;
        }

        let primary = touches.iter()
            .filter(|t| t.pointer_id == 0)
            .next()
            .or_else(|| touches.first())
            .copied();

        let velocity = primary.map(|p| self.velocity_estimator.update(p)).unwrap_or_default();
        let accelerated = self.acceleration.apply(velocity);

        let delta = MouseMoveEvent {
            delta_x: accelerated.x as i32,
            delta_y: accelerated.y as i32,
        };

        if delta.delta_x != 0 || delta.delta_y != 0 {
            self.movement_buffer.push_back(delta);
            self.coalesce_buffer();
        }

        if self.last_emit.elapsed() >= self.target_interval {
            self.last_emit = Instant::now();
            self.emit_coalesced()
        } else {
            None
        }
    }

    fn coalesce_buffer(&mut self) {
        while self.movement_buffer.len() > 1 {
            let len = self.movement_buffer.len();
            let last = self.movement_buffer[len - 1];
            let prev = self.movement_buffer[len - 2];

            if last.delta_x.abs() < 3 && last.delta_y.abs() < 3 &&
               prev.delta_x.abs() < 3 && prev.delta_y.abs() < 3 {
                let merged = MouseMoveEvent {
                    delta_x: prev.delta_x + last.delta_x,
                    delta_y: prev.delta_y + last.delta_y,
                };
                self.movement_buffer.pop_back();
                self.movement_buffer.pop_back();
                self.movement_buffer.push_back(merged);
            } else {
                break;
            }
        }
    }

    fn emit_coalesced(&mut self) -> Option<InputEvent> {
        if self.movement_buffer.is_empty() {
            return None;
        }

        let total = self.movement_buffer.iter().fold(
            MouseMoveEvent { delta_x: 0, delta_y: 0 },
            |acc, m| MouseMoveEvent {
                delta_x: acc.delta_x + m.delta_x,
                delta_y: acc.delta_y + m.delta_y,
            }
        );
        self.movement_buffer.clear();

        self.sequence_number += 1;
        Some(InputEvent::MouseMove {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            delta_x: total.delta_x,
            delta_y: total.delta_y,
        })
    }

    pub fn flush(&mut self) -> Option<InputEvent> {
        if self.movement_buffer.is_empty() {
            return None;
        }
        self.last_emit = Instant::now();
        self.emit_coalesced()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GestureType { None, Pan, Pinch, Scroll, Tap }

struct GestureClassifier {
    last_touches: Vec<TouchPoint>,
}

impl GestureClassifier {
    fn new() -> Self {
        Self { last_touches: Vec::new() }
    }

    fn classify(&mut self, touches: &[TouchPoint]) -> GestureType {
        self.last_touches = touches.to_vec();
        
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

#[derive(Debug, Clone, Copy, Default)]
struct Velocity { x: f32, y: f32 }

struct VelocityEstimator {
    last_pos: Option<(f32, f32)>,
    last_time: Option<Instant>,
    velocity: Velocity,
}

impl VelocityEstimator {
    fn new() -> Self {
        Self { last_pos: None, last_time: None, velocity: Velocity::default() }
    }

    fn update(&mut self, point: TouchPoint) -> Velocity {
        let now = point.timestamp;
        if let (Some((lx, ly)), Some(lt)) = (self.last_pos, self.last_time) {
            let dt = now.duration_since(lt).as_secs_f32();
            if dt > 0.0 {
                self.velocity.x = (point.x - lx) / dt;
                self.velocity.y = (point.y - ly) / dt;
            }
        }
        self.last_pos = Some((point.x, point.y));
        self.last_time = Some(now);
        self.velocity
    }
}

#[derive(Debug, Clone, Copy)]
struct AccelerationCurve {
    threshold: f32,
    acceleration: f32,
}

impl Default for AccelerationCurve {
    fn default() -> Self {
        Self { threshold: 100.0, acceleration: 1.5 }
    }
}

impl AccelerationCurve {
    fn apply(&self, velocity: Velocity) -> Velocity {
        let speed = (velocity.x.powi(2) + velocity.y.powi(2)).sqrt();
        if speed > self.threshold {
            let factor = 1.0 + (speed - self.threshold) / self.threshold * self.acceleration;
            Velocity { x: velocity.x * factor, y: velocity.y * factor }
        } else {
            velocity
        }
    }
}