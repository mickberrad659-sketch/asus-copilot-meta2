use evdev::{EventType, InputEvent, KeyCode};

const META: u16 = KeyCode::KEY_LEFTMETA.code();
const SHIFT: u16 = KeyCode::KEY_LEFTSHIFT.code();
const F23: u16 = KeyCode::KEY_F23.code();
const F24: u16 = KeyCode::KEY_F24.code();

#[derive(Default)]
pub struct CopilotFilter {
    pending: Vec<InputEvent>,
    copilot_held: bool,
}

impl CopilotFilter {
    pub fn frame(&mut self, events: impl IntoIterator<Item = InputEvent>) -> Vec<InputEvent> {
        let mut output = Vec::new();

        for event in events {
            if event.event_type() != EventType::KEY {
                continue;
            }

            let (code, value) = (event.code(), event.value());
            if self.copilot_held {
                if code == F23 || code == SHIFT || code == META {
                    if code == F23 && value == 0 {
                        output.push(key(F24, 0));
                    }
                    if code == META && value == 0 {
                        self.copilot_held = false;
                    }
                    continue;
                }
                output.push(event);
                continue;
            }

            match self.pending.as_slice() {
                [] if code == META && value == 1 => self.pending.push(event),
                [meta] if meta.code() == META && code == SHIFT && value == 1 => {
                    self.pending.push(event)
                }
                [meta, shift]
                    if meta.code() == META
                        && shift.code() == SHIFT
                        && code == F23
                        && value == 1 =>
                {
                    self.pending.clear();
                    self.copilot_held = true;
                    output.push(key(F24, 1));
                }
                _ => {
                    output.append(&mut self.pending);
                    output.push(event);
                }
            }
        }

        output
    }
}

fn key(code: u16, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(events: &[InputEvent]) -> Vec<(u16, i32)> {
        events.iter().map(|e| (e.code(), e.value())).collect()
    }

    #[test]
    fn replaces_copilot_chord_with_f24() {
        let mut filter = CopilotFilter::default();
        let down = filter.frame([key(META, 1), key(SHIFT, 1), key(F23, 1)]);
        let up = filter.frame([key(F23, 0), key(SHIFT, 0), key(META, 0)]);
        assert_eq!(codes(&down), [(F24, 1)]);
        assert_eq!(codes(&up), [(F24, 0)]);
    }

    #[test]
    fn replaces_copilot_chord_split_across_kernel_frames() {
        let mut filter = CopilotFilter::default();
        assert!(filter.frame([key(META, 1)]).is_empty());
        assert!(filter.frame([key(SHIFT, 1)]).is_empty());
        assert_eq!(codes(&filter.frame([key(F23, 1)])), [(F24, 1)]);
        assert_eq!(codes(&filter.frame([key(F23, 0)])), [(F24, 0)]);
        assert!(filter.frame([key(SHIFT, 0)]).is_empty());
        assert!(filter.frame([key(META, 0)]).is_empty());
    }

    #[test]
    fn ordinary_shortcuts_are_unchanged() {
        let mut filter = CopilotFilter::default();
        let events = [key(META, 1), key(SHIFT, 1), key(KeyCode::KEY_A.code(), 1)];
        assert_eq!(codes(&filter.frame(events)), codes(&events));
    }

    #[test]
    fn ordinary_shortcut_split_across_frames_is_unchanged() {
        let mut filter = CopilotFilter::default();
        assert!(filter.frame([key(META, 1)]).is_empty());
        assert!(filter.frame([key(SHIFT, 1)]).is_empty());
        let a = key(KeyCode::KEY_A.code(), 1);
        assert_eq!(
            codes(&filter.frame([a])),
            [(META, 1), (SHIFT, 1), (KeyCode::KEY_A.code(), 1)]
        );
    }

    #[test]
    fn keys_pressed_while_modifier_is_held_are_forwarded() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1), key(SHIFT, 1), key(F23, 1)]);
        let a = key(KeyCode::KEY_A.code(), 1);
        assert_eq!(codes(&filter.frame([a])), codes(&[a]));
    }
}
