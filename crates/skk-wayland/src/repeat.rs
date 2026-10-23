use std::time::{Duration, Instant};

use skk_core::KeyEvent;

pub struct Repeat {
    pub keycode: u32,
    pub event: Option<KeyEvent>,
    pub forwarded: bool,
    pub time: u32,
    pub last: Instant,
    pub next: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RepeatAction {
    Forward { keycode: u32, time: u32 },
    Engine(KeyEvent),
}

impl Repeat {
    pub fn tick(&mut self, now: Instant, interval: Duration) -> Option<RepeatAction> {
        if now < self.next {
            return None;
        }
        self.time = self
            .time
            .wrapping_add(now.duration_since(self.last).as_millis() as u32);
        self.last = now;
        self.next = now + interval;
        if self.forwarded {
            Some(RepeatAction::Forward {
                keycode: self.keycode,
                time: self.time,
            })
        } else {
            self.event.clone().map(RepeatAction::Engine)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skk_core::Key;

    #[test]
    fn repeat_tick_preserves_forwarding_route_and_respects_delay() {
        let start = Instant::now();
        let event = KeyEvent::new(Key::Char('a'));
        let mut forwarded = Repeat {
            keycode: 38,
            event: None,
            forwarded: true,
            time: 100,
            last: start,
            next: start + Duration::from_millis(10),
        };
        assert_eq!(
            forwarded.tick(start + Duration::from_millis(5), Duration::from_millis(20)),
            None
        );
        assert_eq!(
            forwarded.tick(start + Duration::from_millis(12), Duration::from_millis(20)),
            Some(RepeatAction::Forward {
                keycode: 38,
                time: 112,
            })
        );

        let mut handled = Repeat {
            keycode: 38,
            event: Some(event.clone()),
            forwarded: false,
            time: 100,
            last: start,
            next: start,
        };
        assert_eq!(
            handled.tick(start + Duration::from_millis(15), Duration::from_millis(20)),
            Some(RepeatAction::Engine(event))
        );
    }
}
