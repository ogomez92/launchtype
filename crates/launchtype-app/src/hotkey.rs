//! Global hotkey (Ctrl+Alt+Space) via the global-hotkey crate. Events land
//! on a crossbeam channel; a 50ms wx timer drains it on the UI thread.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use wxdragon::timer::Timer;
use wxdragon::widgets::frame::Frame;

pub struct Hotkey {
    _manager: GlobalHotKeyManager,
    _timer: Timer<Frame>,
}

pub fn register(frame: &Frame, on_press: impl Fn() + 'static) -> Result<Hotkey, String> {
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    manager.register(hotkey).map_err(|e| e.to_string())?;

    let receiver = GlobalHotKeyEvent::receiver();
    let timer = Timer::new(frame);
    timer.on_tick(move |_| {
        while let Ok(event) = receiver.try_recv() {
            if event.state() == HotKeyState::Pressed {
                on_press();
            }
        }
    });
    timer.start(50, false);

    Ok(Hotkey { _manager: manager, _timer: timer })
}
