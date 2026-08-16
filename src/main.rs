use anyhow::{Context, Result, bail};
use asus_copilot_meta2::CopilotFilter;
use evdev::{AttributeSet, Device, KeyCode, RelativeAxisCode, uinput::VirtualDevice};
use std::{env, path::PathBuf};

const DEFAULT_DEVICE: &str = "/dev/input/by-path/platform-i8042-serio-0-event-kbd";

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

    source
        .grab()
        .with_context(|| format!("cannot grab {}", path.display()))?;
    eprintln!(
        "remapping ASUS Copilot key from {}; stop the process to release the keyboard",
        path.display()
    );

    let mut filter = CopilotFilter::default();
    loop {
        let frame: Vec<_> = source.fetch_events()?.collect();
        let filtered = filter.frame(frame);
        if !filtered.keyboard.is_empty() {
            keyboard.emit(&filtered.keyboard)?;
        }
        if !filtered.pointer.is_empty() {
            pointer.emit(&filtered.pointer)?;
        }
    }
}
