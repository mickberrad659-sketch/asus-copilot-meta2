use evdev::{EventType, InputEvent, KeyCode};

const META: u16 = KeyCode::KEY_LEFTMETA.code();
const SHIFT: u16 = KeyCode::KEY_LEFTSHIFT.code();
const J: u16 = KeyCode::KEY_J.code();
const K: u16 = KeyCode::KEY_K.code();
const N: u16 = KeyCode::KEY_N.code();
const F23: u16 = KeyCode::KEY_F23.code();
const F24: u16 = KeyCode::KEY_F24.code();

#[derive(Default)]
enum Active {
    #[default]
    None,
    Copilot,
    Pointer {
        held: Option<(u16, u16)>,
        meta_down: bool,
    },
    Scroll {
        meta_down: bool,
    },
    ForwardedMeta,
}

#[derive(Default)]
pub struct FilteredFrame {
    pub keyboard: Vec<InputEvent>,
    pub pointer: Vec<InputEvent>,
    pub scroll_mode: Option<bool>,
}

#[derive(Default)]
pub struct CopilotFilter {
    pending: Vec<InputEvent>,
    active: Active,
}

impl CopilotFilter {
    pub fn frame(&mut self, events: impl IntoIterator<Item = InputEvent>) -> FilteredFrame {
        let mut output = FilteredFrame::default();

        for event in events {
            if event.event_type() != EventType::KEY {
                continue;
            }

            let (code, value) = (event.code(), event.value());
            match &mut self.active {
                Active::Copilot => {
                    if code == F23 || code == SHIFT || code == META {
                        if code == F23 && value == 0 {
                            output.keyboard.push(key(F24, 0));
                        }
                        if code == META && value == 0 {
                            self.active = Active::None;
                        }
                    } else {
                        output.keyboard.push(event);
                    }
                    continue;
                }
                Active::Pointer { held, meta_down } => {
                    if code == META {
                        if value == 0 {
                            *meta_down = false;
                            if held.is_none() {
                                self.active = Active::None;
                            }
                        }
                    } else if let Some((trigger, button)) = *held {
                        if code == trigger && value == 0 {
                            output.pointer.push(key(button, 0));
                            *held = None;
                            if !*meta_down {
                                self.active = Active::None;
                            }
                        } else if code != trigger {
                            output.keyboard.push(event);
                        }
                    } else if is_pointer_trigger(code) && value == 1 {
                        let button = pointer_button(code);
                        *held = Some((code, button));
                        output.pointer.push(key(button, 1));
                    } else if code == N && value == 1 {
                        self.active = Active::Scroll {
                            meta_down: *meta_down,
                        };
                        output.scroll_mode = Some(true);
                    } else {
                        if *meta_down {
                            output.keyboard.push(key(META, 1));
                            self.active = Active::ForwardedMeta;
                        } else {
                            self.active = Active::None;
                        }
                        output.keyboard.push(event);
                    }
                    continue;
                }
                Active::Scroll { meta_down } => {
                    if code == META && value == 0 {
                        *meta_down = false;
                    } else if code == N && value == 0 {
                        output.scroll_mode = Some(false);
                        self.active = if *meta_down {
                            Active::Pointer {
                                held: None,
                                meta_down: true,
                            }
                        } else {
                            Active::None
                        };
                    } else if code != N {
                        output.keyboard.push(event);
                    }
                    continue;
                }
                Active::ForwardedMeta => {
                    if code == META && value == 0 {
                        output.keyboard.push(event);
                        self.active = Active::None;
                    } else if is_pointer_trigger(code) && value == 1 {
                        output.keyboard.push(key(META, 0));
                        let button = pointer_button(code);
                        output.pointer.push(key(button, 1));
                        self.active = Active::Pointer {
                            held: Some((code, button)),
                            meta_down: true,
                        };
                    } else if code == N && value == 1 {
                        output.keyboard.push(key(META, 0));
                        output.scroll_mode = Some(true);
                        self.active = Active::Scroll { meta_down: true };
                    } else {
                        output.keyboard.push(event);
                    }
                    continue;
                }
                Active::None => {}
            }

            match self.pending.as_slice() {
                [] if code == META && value == 1 => self.pending.push(event),
                [meta] if meta.code() == META && code == SHIFT && value == 1 => {
                    self.pending.push(event)
                }
                [meta] if meta.code() == META && is_pointer_trigger(code) && value == 1 => {
                    let button = pointer_button(code);
                    self.pending.clear();
                    self.active = Active::Pointer {
                        held: Some((code, button)),
                        meta_down: true,
                    };
                    output.pointer.push(key(button, 1));
                }
                [meta] if meta.code() == META && code == N && value == 1 => {
                    self.pending.clear();
                    self.active = Active::Scroll { meta_down: true };
                    output.scroll_mode = Some(true);
                }
                [meta, shift]
                    if meta.code() == META
                        && shift.code() == SHIFT
                        && code == F23
                        && value == 1 =>
                {
                    self.pending.clear();
                    self.active = Active::Copilot;
                    output.keyboard.push(key(F24, 1));
                }
                _ => {
                    let forwarded_meta = self.pending.first().is_some_and(|e| e.code() == META)
                        && !(code == META && value == 0);
                    output.keyboard.append(&mut self.pending);
                    output.keyboard.push(event);
                    if forwarded_meta {
                        self.active = Active::ForwardedMeta;
                    }
                }
            }
        }

        output
    }
}

fn key(code: u16, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code, value)
}

fn pointer_button(trigger: u16) -> u16 {
    if trigger == J {
        KeyCode::BTN_LEFT.code()
    } else {
        KeyCode::BTN_RIGHT.code()
    }
}

fn is_pointer_trigger(code: u16) -> bool {
    code == J || code == K
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(events: &[InputEvent]) -> Vec<(u16, i32)> {
        events.iter().map(|e| (e.code(), e.value())).collect()
    }

    #[test]
    fn replaces_copilot_chord_split_across_frames() {
        let mut filter = CopilotFilter::default();
        assert!(filter.frame([key(META, 1)]).keyboard.is_empty());
        assert!(filter.frame([key(SHIFT, 1)]).keyboard.is_empty());
        assert_eq!(codes(&filter.frame([key(F23, 1)]).keyboard), [(F24, 1)]);
        assert_eq!(codes(&filter.frame([key(F23, 0)]).keyboard), [(F24, 0)]);
        assert!(filter.frame([key(SHIFT, 0)]).keyboard.is_empty());
        assert!(filter.frame([key(META, 0)]).keyboard.is_empty());
    }

    #[test]
    fn ordinary_shortcut_split_across_frames_is_unchanged() {
        let mut filter = CopilotFilter::default();
        assert!(filter.frame([key(META, 1)]).keyboard.is_empty());
        let a = key(KeyCode::KEY_A.code(), 1);
        assert_eq!(
            codes(&filter.frame([a]).keyboard),
            [(META, 1), (KeyCode::KEY_A.code(), 1)]
        );
    }

    #[test]
    fn super_j_is_a_holdable_left_button_without_leaking_super() {
        let mut filter = CopilotFilter::default();
        assert!(filter.frame([key(META, 1)]).keyboard.is_empty());
        let down = filter.frame([key(J, 1)]);
        assert!(down.keyboard.is_empty());
        assert_eq!(codes(&down.pointer), [(KeyCode::BTN_LEFT.code(), 1)]);

        let up = filter.frame([key(J, 0)]);
        assert_eq!(codes(&up.pointer), [(KeyCode::BTN_LEFT.code(), 0)]);
        assert!(up.keyboard.is_empty());
        assert!(filter.frame([key(META, 0)]).keyboard.is_empty());
    }

    #[test]
    fn releasing_super_before_j_never_restores_or_sticks_it() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        filter.frame([key(J, 1)]);
        let meta_up = filter.frame([key(META, 0)]);
        assert!(meta_up.keyboard.is_empty());
        let j_up = filter.frame([key(J, 0)]);
        assert!(j_up.keyboard.is_empty());
        assert_eq!(codes(&j_up.pointer), [(KeyCode::BTN_LEFT.code(), 0)]);
    }

    #[test]
    fn super_k_is_a_right_button_and_repeat_does_not_reclic() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        let down = filter.frame([key(K, 1), key(K, 2)]);
        assert!(down.keyboard.is_empty());
        assert_eq!(codes(&down.pointer), [(KeyCode::BTN_RIGHT.code(), 1)]);
        let up = filter.frame([key(K, 0), key(META, 0)]);
        assert_eq!(codes(&up.pointer), [(KeyCode::BTN_RIGHT.code(), 0)]);
        assert!(up.keyboard.is_empty());
    }

    #[test]
    fn super_n_enables_scroll_mode_until_n_is_released() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        let down = filter.frame([key(N, 1)]);
        assert!(down.keyboard.is_empty());
        assert_eq!(down.scroll_mode, Some(true));
        let up = filter.frame([key(N, 0)]);
        assert!(up.keyboard.is_empty());
        assert_eq!(up.scroll_mode, Some(false));
        assert!(filter.frame([key(META, 0)]).keyboard.is_empty());
    }

    #[test]
    fn repeated_j_presses_remain_clicks_until_super_is_released() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        let first_down = filter.frame([key(J, 1)]);
        let first_up = filter.frame([key(J, 0)]);
        let second_down = filter.frame([key(J, 1)]);
        let second_up = filter.frame([key(J, 0)]);
        let meta_up = filter.frame([key(META, 0)]);

        assert_eq!(codes(&first_down.pointer), [(KeyCode::BTN_LEFT.code(), 1)]);
        assert_eq!(codes(&first_up.pointer), [(KeyCode::BTN_LEFT.code(), 0)]);
        assert_eq!(codes(&second_down.pointer), [(KeyCode::BTN_LEFT.code(), 1)]);
        assert_eq!(codes(&second_up.pointer), [(KeyCode::BTN_LEFT.code(), 0)]);
        assert!(first_down.keyboard.is_empty());
        assert!(first_up.keyboard.is_empty());
        assert!(second_down.keyboard.is_empty());
        assert!(second_up.keyboard.is_empty());
        assert!(meta_up.keyboard.is_empty());
    }

    #[test]
    fn another_key_after_click_restores_an_ordinary_super_shortcut() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        filter.frame([key(J, 1)]);
        filter.frame([key(J, 0)]);
        let a = filter.frame([key(KeyCode::KEY_A.code(), 1)]);
        assert_eq!(codes(&a.keyboard), [(META, 1), (KeyCode::KEY_A.code(), 1)]);
        assert_eq!(codes(&filter.frame([key(META, 0)]).keyboard), [(META, 0)]);
    }

    #[test]
    fn pointer_mode_can_resume_after_another_super_shortcut() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        filter.frame([key(J, 1)]);
        filter.frame([key(J, 0)]);

        let h = filter.frame([key(KeyCode::KEY_H.code(), 1)]);
        assert_eq!(codes(&h.keyboard), [(META, 1), (KeyCode::KEY_H.code(), 1)]);
        assert_eq!(
            codes(&filter.frame([key(KeyCode::KEY_H.code(), 0)]).keyboard),
            [(KeyCode::KEY_H.code(), 0)]
        );

        let j = filter.frame([key(J, 1)]);
        assert_eq!(codes(&j.keyboard), [(META, 0)]);
        assert_eq!(codes(&j.pointer), [(KeyCode::BTN_LEFT.code(), 1)]);
        assert_eq!(
            codes(&filter.frame([key(J, 0)]).pointer),
            [(KeyCode::BTN_LEFT.code(), 0)]
        );
        assert!(filter.frame([key(META, 0)]).keyboard.is_empty());
    }

    #[test]
    fn scroll_mode_can_start_after_another_super_shortcut() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        filter.frame([key(KeyCode::KEY_H.code(), 1)]);
        filter.frame([key(KeyCode::KEY_H.code(), 0)]);
        let n = filter.frame([key(N, 1)]);
        assert_eq!(codes(&n.keyboard), [(META, 0)]);
        assert_eq!(n.scroll_mode, Some(true));
    }

    #[test]
    fn keys_typed_while_pointer_is_held_are_forwarded_without_super() {
        let mut filter = CopilotFilter::default();
        filter.frame([key(META, 1)]);
        filter.frame([key(J, 1)]);
        let a = key(KeyCode::KEY_A.code(), 1);
        assert_eq!(codes(&filter.frame([a]).keyboard), codes(&[a]));
    }
}
