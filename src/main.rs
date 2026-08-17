use anyhow::{Context, Result, bail};
use asus_copilot_meta2::CopilotFilter;
use evdev::{
    AbsoluteAxisCode, AttributeSet, Device, EventType, InputEvent, KeyCode, LedCode,
    RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};
use std::{
    collections::HashSet,
    env,
    os::fd::AsRawFd,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

const DEFAULT_DEVICE: &str = "/dev/input/by-path/platform-i8042-serio-0-event-kbd";
const TOUCHPAD_NAME: &str = "ASUF1209:00 2808:0219 Touchpad";
const CLICK_GUARD: Duration = Duration::from_millis(250);
const LAYOUT_SYNC: Duration = Duration::from_millis(500);
const MT_TOOL_FINGER: i32 = 0;
const MT_TOOL_PALM: i32 = 2;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => run(args.next().map(PathBuf::from)),
        Some("doctor") => doctor(args.next().map(PathBuf::from)),
        _ => {
            eprintln!("usage: asus-copilot-meta2 <run|doctor> [input-device]");
            std::process::exit(2);
        }
    }
}

fn open(path: Option<PathBuf>) -> Result<(PathBuf, Device)> {
    let path = path.unwrap_or_else(|| PathBuf::from(DEFAULT_DEVICE));
    let device = Device::open(&path).with_context(|| {
        format!(
            "cannot open {}; check input-group/udev permissions",
            path.display()
        )
    })?;
    Ok((path, device))
}

fn doctor(path: Option<PathBuf>) -> Result<()> {
    let (path, device) = open(path)?;
    let keys = device
        .supported_keys()
        .context("device does not report keyboard keys")?;
    if !keys.contains(KeyCode::KEY_F23)
        || !keys.contains(KeyCode::KEY_LEFTMETA)
        || !keys.contains(KeyCode::KEY_LEFTSHIFT)
    {
        bail!("{} is not the expected ASUS keyboard", path.display());
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .context("cannot write /dev/uinput; check udev permissions")?;
    println!(
        "ok: {} ({})",
        path.display(),
        device.name().unwrap_or("unnamed keyboard")
    );
    Ok(())
}

fn run(path: Option<PathBuf>) -> Result<()> {
    let (path, mut source) = open(path)?;
    let mut keys: AttributeSet<KeyCode> = source
        .supported_keys()
        .context("device does not report keyboard keys")?
        .into_iter()
        .collect();
    keys.insert(KeyCode::KEY_F24);

    let mut keyboard = VirtualDevice::builder()
        .context("cannot open /dev/uinput")?
        .name("ASUS Copilot Meta2 Keyboard")
        .with_keys(&keys)?
        .build()?;

    let pointer_buttons = AttributeSet::from_iter([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT]);
    let pointer_axes = AttributeSet::from_iter([RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y]);
    let mut pointer = VirtualDevice::builder()
        .context("cannot open /dev/uinput for pointer")?
        .name("ASUS Meta Pointer")
        .with_keys(&pointer_buttons)?
        .with_relative_axes(&pointer_axes)?
        .build()?;

    let touchpad_path = find_touchpad()?;
    let mut touchpad_source = Device::open(&touchpad_path)
        .with_context(|| format!("cannot open {}", touchpad_path.display()))?;
    let mut touchpad = clone_touchpad(&touchpad_source)?;

    source
        .grab()
        .with_context(|| format!("cannot grab {}", path.display()))?;
    touchpad_source
        .grab()
        .with_context(|| format!("cannot grab {}", touchpad_path.display()))?;
    source.set_nonblocking(true)?;
    touchpad_source.set_nonblocking(true)?;
    let mut russian_layout = current_layout_is_russian().unwrap_or(false);
    set_caps_led(&mut source, russian_layout)?;
    eprintln!(
        "remapping keyboard {} and guarding touchpad clicks from {} ({} ms)",
        path.display(),
        touchpad_path.display(),
        CLICK_GUARD.as_millis()
    );

    let mut filter = CopilotFilter::default();
    let mut last_typing: Option<Instant> = None;
    let mut meta_down = false;
    let mut touchpad_guard = TouchpadGuard::default();
    let mut touchpad_frame = Vec::new();
    let mut last_layout_sync = Instant::now();
    loop {
        let mut fds = [
            libc::pollfd {
                fd: source.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: touchpad_source.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 250) };
        if ready < 0 {
            return Err(std::io::Error::last_os_error()).context("polling input devices");
        }

        if fds[0].revents & libc::POLLIN != 0 {
            let frame: Vec<_> = source.fetch_events()?.collect();
            for event in &frame {
                if event.event_type() != EventType::KEY {
                    continue;
                }
                if event.code() == KeyCode::KEY_LEFTMETA.code() {
                    meta_down = event.value() != 0;
                    continue;
                }
                if event.value() == 1 || event.value() == 2 {
                    let modifier = matches!(event.code(),
                        x if x == KeyCode::KEY_LEFTSHIFT.code()
                          || x == KeyCode::KEY_RIGHTSHIFT.code()
                          || x == KeyCode::KEY_LEFTCTRL.code()
                          || x == KeyCode::KEY_RIGHTCTRL.code()
                          || x == KeyCode::KEY_LEFTALT.code()
                          || x == KeyCode::KEY_RIGHTALT.code()
                          || x == KeyCode::KEY_RIGHTMETA.code());
                    let meta_pointer = meta_down
                        && matches!(event.code(),
                        x if x == KeyCode::KEY_J.code() || x == KeyCode::KEY_K.code());
                    if !modifier && !meta_pointer {
                        last_typing = Some(Instant::now());
                    }
                }
            }
            let filtered = filter.frame(frame);
            if !filtered.keyboard.is_empty() {
                keyboard.emit(&filtered.keyboard)?;
            }
            if !filtered.pointer.is_empty() {
                pointer.emit(&filtered.pointer)?;
            }
        }

        if fds[1].revents & libc::POLLIN != 0 {
            let guarded = last_typing.is_some_and(|at| at.elapsed() < CLICK_GUARD);
            for event in touchpad_source.fetch_events()? {
                if event.event_type() == EventType::SYNCHRONIZATION {
                    if event.code() == 0 {
                        forward_touchpad_frame(
                            &mut touchpad,
                            std::mem::take(&mut touchpad_frame),
                            guarded,
                            &mut touchpad_guard,
                        )?;
                    }
                } else if event.event_type() != EventType::MISC {
                    touchpad_frame.push(event);
                }
            }
        }

        if last_layout_sync.elapsed() >= LAYOUT_SYNC {
            if let Some(actual_russian) = current_layout_is_russian()
                && actual_russian != russian_layout
            {
                russian_layout = actual_russian;
                set_caps_led(&mut source, russian_layout)?;
            }
            last_layout_sync = Instant::now();
        }
    }
}

fn current_layout_is_russian() -> Option<bool> {
    let output = Command::new("niri")
        .args(["msg", "-j", "keyboard-layouts"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let value = text.split("\"current_idx\":").nth(1)?;
    let index: usize = value
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some(index == 1)
}

fn set_caps_led(keyboard: &mut Device, enabled: bool) -> Result<()> {
    keyboard
        .send_events(&[
            InputEvent::new(EventType::LED.0, LedCode::LED_CAPSL.0, i32::from(enabled)),
            InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0),
        ])
        .context("updating the physical Caps Lock LED")
}

fn forward_touchpad_frame(
    touchpad: &mut VirtualDevice,
    raw: Vec<evdev::InputEvent>,
    guarded: bool,
    guard: &mut TouchpadGuard,
) -> Result<()> {
    let frame = guard.frame(raw, guarded);
    if !frame.is_empty() {
        touchpad.emit(&frame)?;
    }
    Ok(())
}

#[derive(Default)]
struct TouchpadGuard {
    current_slot: i32,
    palm_slots: HashSet<i32>,
    suppressed_left: bool,
}

impl TouchpadGuard {
    fn frame(&mut self, raw: Vec<InputEvent>, guarded: bool) -> Vec<InputEvent> {
        let mut frame = Vec::with_capacity(raw.len() + 2);
        let slot = AbsoluteAxisCode::ABS_MT_SLOT.0;
        let tracking_id = AbsoluteAxisCode::ABS_MT_TRACKING_ID.0;
        let tool_type = AbsoluteAxisCode::ABS_MT_TOOL_TYPE.0;

        for event in raw {
            if event.event_type() == EventType::ABSOLUTE && event.code() == slot {
                self.current_slot = event.value();
                frame.push(event);
                continue;
            }
            if event.event_type() == EventType::ABSOLUTE && event.code() == tracking_id {
                if event.value() >= 0 {
                    // Keep a whole multi-finger gesture inhibited when its
                    // first contact began inside the 250 ms guard window.
                    if guarded || !self.palm_slots.is_empty() {
                        self.palm_slots.insert(self.current_slot);
                    }
                    frame.push(event);
                    let contact_type = if self.palm_slots.contains(&self.current_slot) {
                        MT_TOOL_PALM
                    } else {
                        // MT_TOOL_TYPE is slot state and survives tracking-id
                        // changes. Explicitly reset a reused slot to FINGER.
                        MT_TOOL_FINGER
                    };
                    frame.push(InputEvent::new(
                        EventType::ABSOLUTE.0,
                        tool_type,
                        contact_type,
                    ));
                } else {
                    frame.push(event);
                    self.palm_slots.remove(&self.current_slot);
                }
                continue;
            }
            if event.event_type() == EventType::ABSOLUTE
                && event.code() == tool_type
                && self.palm_slots.contains(&self.current_slot)
            {
                frame.push(InputEvent::new(
                    EventType::ABSOLUTE.0,
                    tool_type,
                    MT_TOOL_PALM,
                ));
                continue;
            }
            if event.event_type() == EventType::KEY && event.code() == KeyCode::BTN_LEFT.code() {
                if event.value() == 1 && guarded {
                    self.suppressed_left = true;
                    continue;
                }
                if event.value() == 0 && self.suppressed_left {
                    self.suppressed_left = false;
                    continue;
                }
            }
            frame.push(event);
        }
        frame
    }
}

#[cfg(test)]
mod touchpad_tests {
    use super::*;

    fn abs(code: AbsoluteAxisCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::ABSOLUTE.0, code.0, value)
    }

    #[test]
    fn guarded_contact_is_marked_as_palm_but_release_is_preserved() {
        let mut guard = TouchpadGuard::default();
        let down = guard.frame(
            vec![
                abs(AbsoluteAxisCode::ABS_MT_SLOT, 0),
                abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, 42),
            ],
            true,
        );
        assert_eq!(down.len(), 3);
        assert_eq!(down[2].code(), AbsoluteAxisCode::ABS_MT_TOOL_TYPE.0);
        assert_eq!(down[2].value(), MT_TOOL_PALM);

        let up = guard.frame(vec![abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, -1)], false);
        assert_eq!(up.len(), 1);
        assert!(guard.palm_slots.is_empty());
    }

    #[test]
    fn ordinary_multitouch_frames_are_byte_for_byte_unchanged() {
        let mut guard = TouchpadGuard::default();
        let raw = vec![
            abs(AbsoluteAxisCode::ABS_MT_SLOT, 1),
            abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, 7),
            abs(AbsoluteAxisCode::ABS_MT_POSITION_X, 1234),
        ];
        let output = guard.frame(raw.clone(), false);
        assert_eq!(&output[..2], &raw[..2]);
        assert_eq!(output[2].code(), AbsoluteAxisCode::ABS_MT_TOOL_TYPE.0);
        assert_eq!(output[2].value(), MT_TOOL_FINGER);
        assert_eq!(output[3], raw[2]);
    }

    #[test]
    fn slot_is_reset_to_finger_after_a_guarded_palm_contact() {
        let mut guard = TouchpadGuard::default();
        guard.frame(vec![abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, 1)], true);
        guard.frame(vec![abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, -1)], false);
        let next = guard.frame(vec![abs(AbsoluteAxisCode::ABS_MT_TRACKING_ID, 2)], false);
        assert_eq!(next[1].code(), AbsoluteAxisCode::ABS_MT_TOOL_TYPE.0);
        assert_eq!(next[1].value(), MT_TOOL_FINGER);
    }

    #[test]
    fn physical_click_press_and_release_are_suppressed_as_a_pair() {
        let mut guard = TouchpadGuard::default();
        let down = InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.code(), 1);
        let up = InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.code(), 0);
        assert!(guard.frame(vec![down], true).is_empty());
        assert!(guard.frame(vec![up], false).is_empty());
    }
}

fn find_touchpad() -> Result<PathBuf> {
    for index in 0..64 {
        let path = PathBuf::from(format!("/dev/input/event{index}"));
        let Ok(device) = Device::open(&path) else {
            continue;
        };
        if device.name() == Some(TOUCHPAD_NAME) {
            return Ok(path);
        }
    }
    bail!("cannot find touchpad named {TOUCHPAD_NAME}")
}

fn clone_touchpad(source: &Device) -> Result<VirtualDevice> {
    let mut builder = VirtualDevice::builder()?
        .name("ASUS Filtered Touchpad")
        .input_id(source.input_id())
        .with_properties(source.properties())?;
    if let Some(keys) = source.supported_keys() {
        builder = builder.with_keys(keys)?;
    }
    for (axis, info) in source.get_absinfo()? {
        builder = builder.with_absolute_axis(&UinputAbsSetup::new(axis, info))?;
    }
    builder.build().context("creating filtered touchpad")
}
