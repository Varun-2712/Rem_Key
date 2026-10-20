use std::time::{Duration, Instant};
use pir_protocol::events::{ButtonState, EventMetadata, InputEvent, MouseButton};
use uuid::Uuid;
use crate::coalescer::{TouchCoalescer, TouchPoint};

/// Gesture thresholds. Distances are in the phone's touch coordinates (pixels), so the
/// phone should scale them by its screen density.
#[derive(Debug, Clone)]
pub struct TouchpadConfig {
    /// A touch shorter than this that doesn't move beyond `touch_slop` is a tap.
    pub tap_timeout: Duration,
    /// Touching down again this soon after a tap, and moving, drags (tap-and-drag).
    pub double_tap_window: Duration,
    /// Movement below this is finger jitter, not a pan or scroll.
    pub touch_slop: f32,
    /// Two-finger travel that makes one wheel notch.
    pub scroll_px_per_notch: f32,
    /// Content follows the fingers, as on phones and macOS trackpads.
    pub natural_scrolling: bool,
    /// Maximum rate of cursor-move events.
    pub move_hz: u32,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            // libinput: 180 ms; Android: TOUCH_SLOP 8 dp (~24 px at 3x), DOUBLE_TAP_TIMEOUT 300 ms.
            tap_timeout: Duration::from_millis(180),
            double_tap_window: Duration::from_millis(300),
            touch_slop: 24.0,
            scroll_px_per_notch: 40.0,
            natural_scrolling: true,
            move_hz: 120,
        }
    }
}

/// Turns raw multi-touch samples into touchpad input:
///
/// - one finger moves the cursor; a quick tap left-clicks
/// - tap, then touch again and move: drag with the left button held
/// - two fingers scroll; a quick two-finger tap right-clicks
/// - pinches and three or more fingers are recognised and ignored for now
pub struct Touchpad {
    config: TouchpadConfig,
    pan: TouchCoalescer,
    phase: Phase,
    last_tap: Option<(Instant, (f32, f32))>,
    dragging: bool,
    scroll_remainder: (f32, f32),
    sequence_number: u64,
    session_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Idle,
    OneFinger { start: Instant, origin: (f32, f32), moved: bool, drag_armed: bool },
    TwoFingers { start: Instant, origin: (f32, f32), origin_spread: f32, last: (f32, f32), tap_eligible: bool, gesture: TwoFingerGesture },
    /// One finger of a two-finger gesture has lifted; waiting for the other.
    TwoFingersLifting { start: Instant, tap_eligible: bool },
    /// Unsupported gesture: ignore everything until all fingers lift.
    Ignoring,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TwoFingerGesture { Undecided, Scroll, Pinch }

impl Touchpad {
    pub fn new(session_id: Uuid, config: TouchpadConfig) -> Self {
        Self {
            pan: TouchCoalescer::new(session_id, config.move_hz),
            config,
            phase: Phase::Idle,
            last_tap: None,
            dragging: false,
            scroll_remainder: (0.0, 0.0),
            sequence_number: 0,
            session_id,
        }
    }

    /// Feed every touch frame: all fingers currently down, sampled at `now`.
    /// Pass an empty slice when the last finger lifts.
    pub fn on_touches(&mut self, touches: &[TouchPoint], now: Instant) -> Vec<InputEvent> {
        let mut out = Vec::new();
        match touches.len() {
            0 => self.all_lifted(now, &mut out),
            1 => self.one_finger(touches[0], now, &mut out),
            2 => self.two_fingers(touches, now, &mut out),
            _ => {
                self.end_pan(&mut out);
                self.phase = Phase::Ignoring;
            }
        }
        out
    }

    fn one_finger(&mut self, touch: TouchPoint, now: Instant, out: &mut Vec<InputEvent>) {
        let pos = (touch.x, touch.y);
        match self.phase {
            Phase::Idle => {
                let drag_armed = self.last_tap.is_some_and(|(at, tap_pos)| {
                    now.saturating_duration_since(at) <= self.config.double_tap_window
                        && distance(tap_pos, pos) <= self.config.touch_slop * 3.0
                });
                self.pan.discard();
                self.pan.process_touches(&[touch]);
                self.phase = Phase::OneFinger { start: now, origin: pos, moved: false, drag_armed };
            }
            Phase::OneFinger { start, origin, moved, drag_armed } => {
                let now_moved = moved || distance(origin, pos) > self.config.touch_slop;
                if now_moved && !moved && drag_armed {
                    self.dragging = true;
                    out.push(self.button(MouseButton::Left, ButtonState::Pressed));
                }
                // Within the slop the pan stays anchored at the touch-down point, so a tap
                // doesn't nudge the cursor; once it's a pan, that movement is sent too.
                if now_moved {
                    if let Some(event) = self.pan.process_touches(&[touch]) {
                        out.extend(self.restamp(event));
                    }
                }
                self.phase = Phase::OneFinger { start, origin, moved: now_moved, drag_armed };
            }
            Phase::TwoFingers { start, tap_eligible, gesture, .. } => {
                self.phase = Phase::TwoFingersLifting {
                    start,
                    tap_eligible: tap_eligible && gesture == TwoFingerGesture::Undecided,
                };
            }
            Phase::TwoFingersLifting { .. } | Phase::Ignoring => {}
        }
    }

    fn two_fingers(&mut self, touches: &[TouchPoint], now: Instant, out: &mut Vec<InputEvent>) {
        let (a, b) = ((touches[0].x, touches[0].y), (touches[1].x, touches[1].y));
        let centroid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let spread = distance(a, b);
        let fresh = |start, tap_eligible| Phase::TwoFingers {
            start,
            origin: centroid,
            origin_spread: spread,
            last: centroid,
            tap_eligible,
            gesture: TwoFingerGesture::Undecided,
        };

        match self.phase {
            // Fingers rarely land at exactly the same moment; a two-finger tap starts
            // when the first one does.
            Phase::OneFinger { start, moved: false, .. } if !self.dragging => {
                self.pan.discard();
                self.phase = fresh(start, true);
            }
            Phase::Idle => {
                self.pan.discard();
                self.phase = fresh(now, true);
            }
            Phase::OneFinger { .. } | Phase::TwoFingersLifting { .. } => {
                self.end_pan(out);
                self.phase = fresh(now, false);
            }
            Phase::TwoFingers { start, origin, origin_spread, last, tap_eligible, mut gesture } => {
                if gesture == TwoFingerGesture::Undecided {
                    let travel = distance(origin, centroid);
                    let spread_change = (spread - origin_spread).abs();
                    if travel > self.config.touch_slop || spread_change > self.config.touch_slop {
                        gesture = if spread_change > travel { TwoFingerGesture::Pinch } else { TwoFingerGesture::Scroll };
                    }
                }
                if gesture == TwoFingerGesture::Scroll {
                    out.extend(self.scroll_by(centroid.0 - last.0, centroid.1 - last.1));
                }
                self.phase = Phase::TwoFingers { start, origin, origin_spread, last: centroid, tap_eligible, gesture };
            }
            Phase::Ignoring => {}
        }
    }

    fn all_lifted(&mut self, now: Instant, out: &mut Vec<InputEvent>) {
        match self.phase {
            Phase::OneFinger { start, origin, moved, .. } => {
                if self.dragging {
                    self.end_pan(out);
                } else if !moved && now.saturating_duration_since(start) <= self.config.tap_timeout {
                    self.pan.discard();
                    out.push(self.button(MouseButton::Left, ButtonState::Pressed));
                    out.push(self.button(MouseButton::Left, ButtonState::Released));
                    self.last_tap = Some((now, origin));
                    self.phase = Phase::Idle;
                    return;
                } else {
                    self.end_pan(out);
                }
            }
            Phase::TwoFingers { start, tap_eligible, gesture, .. }
                if tap_eligible
                    && gesture == TwoFingerGesture::Undecided
                    && now.saturating_duration_since(start) <= self.config.tap_timeout =>
            {
                self.right_click(out);
            }
            Phase::TwoFingersLifting { start, tap_eligible: true }
                if now.saturating_duration_since(start) <= self.config.tap_timeout =>
            {
                self.right_click(out);
            }
            _ => self.end_pan(out),
        }
        self.last_tap = None;
        self.scroll_remainder = (0.0, 0.0);
        self.phase = Phase::Idle;
    }

    /// Sends any held-back movement and releases a tap-and-drag.
    fn end_pan(&mut self, out: &mut Vec<InputEvent>) {
        if let Some(event) = self.pan.flush() {
            out.extend(self.restamp(event));
        }
        self.pan.discard();
        if self.dragging {
            self.dragging = false;
            out.push(self.button(MouseButton::Left, ButtonState::Released));
        }
    }

    fn right_click(&mut self, out: &mut Vec<InputEvent>) {
        out.push(self.button(MouseButton::Right, ButtonState::Pressed));
        out.push(self.button(MouseButton::Right, ButtonState::Released));
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> Option<InputEvent> {
        // Scroll events count notches with positive = up / right. With natural scrolling
        // the content follows the fingers: fingers down shows what's above (scroll up),
        // fingers right shows what's to the left (scroll left).
        let sign = if self.config.natural_scrolling { 1.0 } else { -1.0 };
        let x = -dx * sign / self.config.scroll_px_per_notch + self.scroll_remainder.0;
        let y = dy * sign / self.config.scroll_px_per_notch + self.scroll_remainder.1;
        let (notches_x, notches_y) = (x.trunc(), y.trunc());
        self.scroll_remainder = (x - notches_x, y - notches_y);
        if notches_x == 0.0 && notches_y == 0.0 {
            return None;
        }
        Some(InputEvent::Scroll {
            metadata: self.next_metadata(),
            delta_x: notches_x as i32,
            delta_y: notches_y as i32,
        })
    }

    fn restamp(&mut self, event: InputEvent) -> Option<InputEvent> {
        match event {
            InputEvent::MouseMove { delta_x, delta_y, .. } => {
                Some(InputEvent::MouseMove { metadata: self.next_metadata(), delta_x, delta_y })
            }
            _ => None,
        }
    }

    fn button(&mut self, button: MouseButton, state: ButtonState) -> InputEvent {
        InputEvent::MouseButton { metadata: self.next_metadata(), button, state }
    }

    fn next_metadata(&mut self) -> EventMetadata {
        self.sequence_number += 1;
        EventMetadata::new(self.session_id, self.sequence_number)
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_A: u32 = 0;
    const ID_B: u32 = 1;

    struct Driver {
        pad: Touchpad,
        t0: Instant,
        events: Vec<InputEvent>,
    }

    impl Driver {
        fn new() -> Self {
            Self { pad: Touchpad::new(Uuid::nil(), TouchpadConfig::default()), t0: Instant::now(), events: Vec::new() }
        }

        fn at(&self, ms: u64) -> Instant {
            self.t0 + Duration::from_millis(ms)
        }

        fn frame(&mut self, ms: u64, fingers: &[(u32, f32, f32)]) {
            let at = self.at(ms);
            let touches: Vec<_> = fingers
                .iter()
                .map(|&(id, x, y)| TouchPoint { x, y, pressure: 1.0, timestamp: at, pointer_id: id })
                .collect();
            let out = self.pad.on_touches(&touches, at);
            self.events.extend(out);
        }

        fn buttons(&self) -> Vec<(MouseButton, ButtonState)> {
            self.events.iter().filter_map(|e| match e {
                InputEvent::MouseButton { button, state, .. } => Some((*button, *state)),
                _ => None,
            }).collect()
        }

        fn moved(&self) -> (i32, i32) {
            self.events.iter().fold((0, 0), |acc, e| match e {
                InputEvent::MouseMove { delta_x, delta_y, .. } => (acc.0 + delta_x, acc.1 + delta_y),
                _ => acc,
            })
        }

        fn scrolled(&self) -> (i32, i32) {
            self.events.iter().fold((0, 0), |acc, e| match e {
                InputEvent::Scroll { delta_x, delta_y, .. } => (acc.0 + delta_x, acc.1 + delta_y),
                _ => acc,
            })
        }
    }

    use ButtonState::{Pressed, Released};
    use MouseButton::{Left, Right};

    #[test]
    fn quick_tap_left_clicks_without_moving() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(40, &[(ID_A, 103.0, 102.0)]);
        d.frame(80, &[]);
        assert_eq!(d.buttons(), vec![(Left, Pressed), (Left, Released)]);
        assert_eq!(d.moved(), (0, 0));
    }

    #[test]
    fn long_press_is_not_a_tap() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(500, &[]);
        assert!(d.buttons().is_empty());
    }

    #[test]
    fn one_finger_drag_moves_cursor_and_does_not_click() {
        let mut d = Driver::new();
        for i in 0..=30 {
            d.frame(i * 10, &[(ID_A, 100.0 + i as f32 * 2.0, 100.0)]);
        }
        d.frame(310, &[]);
        assert!(d.buttons().is_empty());
        let (x, y) = d.moved();
        assert!(x >= 60, "moved {x}");
        assert_eq!(y, 0);
    }

    #[test]
    fn two_finger_tap_right_clicks() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(15, &[(ID_A, 100.0, 100.0), (ID_B, 160.0, 100.0)]);
        d.frame(90, &[(ID_B, 160.0, 100.0)]);
        d.frame(110, &[]);
        assert_eq!(d.buttons(), vec![(Right, Pressed), (Right, Released)]);
    }

    #[test]
    fn two_finger_drag_scrolls_naturally() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0), (ID_B, 160.0, 100.0)]);
        // Fingers move down 200 px: content follows, so the view scrolls up.
        for i in 1..=20 {
            let y = 100.0 + i as f32 * 10.0;
            d.frame(i * 10, &[(ID_A, 100.0, y), (ID_B, 160.0, y)]);
        }
        d.frame(400, &[]);
        let (x, y) = d.scrolled();
        assert_eq!(x, 0);
        assert!(y >= 4, "scrolled {y}");
        assert!(d.buttons().is_empty(), "a scroll must not right-click");
        assert_eq!(d.moved(), (0, 0));
    }

    #[test]
    fn pinch_does_not_scroll() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0), (ID_B, 160.0, 100.0)]);
        for i in 1..=20 {
            let spread = i as f32 * 8.0;
            d.frame(i * 10, &[(ID_A, 100.0 - spread, 100.0), (ID_B, 160.0 + spread, 100.0)]);
        }
        d.frame(400, &[]);
        assert_eq!(d.scrolled(), (0, 0));
        assert!(d.buttons().is_empty());
    }

    #[test]
    fn tap_then_touch_and_move_drags() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(60, &[]);
        d.frame(200, &[(ID_A, 102.0, 101.0)]);
        for i in 1..=20 {
            d.frame(200 + i * 10, &[(ID_A, 102.0 + i as f32 * 5.0, 101.0)]);
        }
        d.frame(500, &[]);
        assert_eq!(d.buttons(), vec![(Left, Pressed), (Left, Released), (Left, Pressed), (Left, Released)]);
        assert!(d.moved().0 > 50);

        // The drag's release comes after its movement.
        let last_move = d.events.iter().rposition(|e| matches!(e, InputEvent::MouseMove { .. })).unwrap();
        let release = d.events.iter().rposition(|e| matches!(e, InputEvent::MouseButton { state: Released, .. })).unwrap();
        assert!(release > last_move);
    }

    #[test]
    fn two_quick_taps_double_click() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(60, &[]);
        d.frame(150, &[(ID_A, 101.0, 100.0)]);
        d.frame(200, &[]);
        assert_eq!(d.buttons(), vec![(Left, Pressed), (Left, Released), (Left, Pressed), (Left, Released)]);
    }

    #[test]
    fn late_second_touch_does_not_drag() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(60, &[]);
        d.frame(1000, &[(ID_A, 100.0, 100.0)]);
        for i in 1..=10 {
            d.frame(1000 + i * 10, &[(ID_A, 100.0 + i as f32 * 5.0, 100.0)]);
        }
        d.frame(1200, &[]);
        assert_eq!(d.buttons(), vec![(Left, Pressed), (Left, Released)]);
    }

    #[test]
    fn three_fingers_are_ignored_and_release_drag() {
        let mut d = Driver::new();
        d.frame(0, &[(ID_A, 100.0, 100.0)]);
        d.frame(60, &[]);
        d.frame(150, &[(ID_A, 100.0, 100.0)]);
        d.frame(200, &[(ID_A, 150.0, 100.0)]);
        d.frame(210, &[(ID_A, 150.0, 100.0), (ID_B, 200.0, 100.0), (2, 250.0, 100.0)]);
        d.frame(400, &[]);
        let buttons = d.buttons();
        assert_eq!(buttons.last(), Some(&(Left, Released)));
        assert_eq!(buttons.iter().filter(|b| b.1 == Pressed).count(), buttons.iter().filter(|b| b.1 == Released).count());
    }
}
