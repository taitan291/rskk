use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::{AsFd, AsRawFd, OwnedFd},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use memmap2::{MmapMut, MmapOptions};
use skk_core::{CandidateWindow, Engine, Key, KeyEvent, Output};
use tempfile::tempfile;
use tracing::{info, warn};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_keyboard,
        wl_registry::WlRegistry,
        wl_seat::WlSeat,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols_misc::{
    zwp_input_method_v2::client::{
        zwp_input_method_keyboard_grab_v2::{Event as GrabEvent, ZwpInputMethodKeyboardGrabV2},
        zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
        zwp_input_method_v2::{Event as InputMethodEvent, ZwpInputMethodV2},
        zwp_input_popup_surface_v2::{Event as PopupEvent, ZwpInputPopupSurfaceV2},
    },
    zwp_virtual_keyboard_v1::client::{
        zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
        zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
    },
};
use xkbcommon::xkb;

use crate::popup::{PopupRenderer, RenderedPopup};

const KEYMAP_FORMAT_XKB_V1: u32 = 1;

struct KeyboardState {
    _context: xkb::Context,
    keymap: xkb::Keymap,
    state: xkb::State,
}

struct Repeat {
    keycode: u32,
    event: Option<KeyEvent>,
    forwarded: bool,
    time: u32,
    last: Instant,
    next: Instant,
}

#[derive(Debug, PartialEq, Eq)]
enum RepeatAction {
    Forward { keycode: u32, time: u32 },
    Engine(KeyEvent),
}

impl Repeat {
    fn tick(&mut self, now: Instant, interval: Duration) -> Option<RepeatAction> {
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

struct ShmBuffer {
    proxy: WlBuffer,
    _mapping: MmapMut,
}

struct State {
    engine: Engine,
    input_method: ZwpInputMethodV2,
    virtual_keyboard: ZwpVirtualKeyboardV1,
    shm: WlShm,
    popup_surface: WlSurface,
    _popup: ZwpInputPopupSurfaceV2,
    popup_buffers: Vec<ShmBuffer>,
    popup_renderer: Option<PopupRenderer>,
    keyboard: Option<KeyboardState>,
    grab: Option<ZwpInputMethodKeyboardGrabV2>,
    active: bool,
    unavailable: bool,
    pending_active: Option<bool>,
    done_serial: u32,
    forwarded_keys: HashSet<u32>,
    repeat_rate: i32,
    repeat_delay: i32,
    repeat: Option<Repeat>,
}

pub fn run(engine: Engine) -> Result<()> {
    let connection = Connection::connect_to_env().context("connect to Wayland display")?;
    let (globals, mut queue) =
        registry_queue_init::<State>(&connection).context("read Wayland globals")?;
    let qh = queue.handle();
    let seat = globals
        .bind::<WlSeat, _, _>(&qh, 1..=9, ())
        .context("compositor does not advertise wl_seat")?;
    let input_manager = globals
        .bind::<ZwpInputMethodManagerV2, _, _>(&qh, 1..=1, ())
        .context("compositor does not support zwp_input_method_manager_v2")?;
    let virtual_manager = globals
        .bind::<ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ())
        .context(
            "compositor does not support zwp_virtual_keyboard_v1; key forwarding requires it",
        )?;
    let compositor = globals
        .bind::<WlCompositor, _, _>(&qh, 1..=6, ())
        .context("compositor does not advertise wl_compositor")?;
    let shm = globals
        .bind::<WlShm, _, _>(&qh, 1..=1, ())
        .context("compositor does not advertise wl_shm")?;
    let input_method = input_manager.get_input_method(&seat, &qh, ());
    let virtual_keyboard = virtual_manager.create_virtual_keyboard(&seat, &qh, ());
    let popup_surface = compositor.create_surface(&qh, ());
    let popup = input_method.get_input_popup_surface(&popup_surface, &qh, ());
    let mut state = State {
        engine,
        input_method,
        virtual_keyboard,
        shm,
        popup_surface,
        _popup: popup,
        popup_buffers: Vec::new(),
        popup_renderer: None,
        keyboard: None,
        grab: None,
        active: false,
        unavailable: false,
        pending_active: None,
        done_serial: 0,
        forwarded_keys: HashSet::new(),
        repeat_rate: 0,
        repeat_delay: 0,
        repeat: None,
    };
    queue
        .roundtrip(&mut state)
        .context("initialize Wayland input method")?;
    if state.unavailable {
        bail!("Wayland compositor reports that another input method owns this seat");
    }
    info!("registered rskk-wayland as a Wayland input method");

    loop {
        if state.fire_repeat(&qh) {
            continue;
        }
        queue.flush().context("flush Wayland requests")?;
        queue
            .dispatch_pending(&mut state)
            .context("dispatch pending Wayland events")?;
        if state.unavailable {
            bail!("Wayland input method became unavailable, likely because of an IME conflict");
        }
        let Some(read_guard) = queue.prepare_read() else {
            continue;
        };
        let timeout = state
            .repeat
            .as_ref()
            .map(|repeat| repeat.next.saturating_duration_since(Instant::now()));
        let timeout_ms = timeout.map_or(-1, |duration| {
            duration.as_millis().min(i32::MAX as u128) as i32
        });
        let mut pollfd = libc::pollfd {
            fd: read_guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // The fd and pollfd stay valid for the duration of poll; the read guard owns the
        // synchronized Wayland socket read until it is either used or dropped.
        let polled = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if polled < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error).context("wait for Wayland events");
        }
        if polled > 0 && pollfd.revents & libc::POLLIN != 0 {
            read_guard.read().context("read Wayland events")?;
        } else {
            drop(read_guard);
        }
        queue
            .dispatch_pending(&mut state)
            .context("dispatch Wayland events")?;
        state.fire_repeat(&qh);
    }
}

impl State {
    fn apply_output(&mut self, output: Output, qh: &QueueHandle<Self>) {
        if !output.consumed {
            return;
        }
        if let Err(error) = self.update_popup(output.candidates.as_ref(), qh) {
            warn!(%error, "could not update candidate popup");
        }
        if let Some(text) = output.commit.filter(|text| !text.is_empty()) {
            self.input_method.commit_string(text);
        }
        let (text, begin, end) = if output.preedit.text.is_empty() {
            (String::new(), -1, -1)
        } else {
            let cursor = output.preedit.cursor.min(i32::MAX as usize) as i32;
            (output.preedit.text, cursor, cursor)
        };
        self.input_method.set_preedit_string(text, begin, end);
        self.input_method.commit(self.done_serial);
    }

    fn update_popup(
        &mut self,
        candidates: Option<&CandidateWindow>,
        qh: &QueueHandle<Self>,
    ) -> Result<()> {
        let Some(candidates) = candidates.filter(|window| !window.items.is_empty()) else {
            self.popup_surface.attach(None, 0, 0);
            self.popup_surface.commit();
            return Ok(());
        };
        let renderer = self.popup_renderer.get_or_insert_with(PopupRenderer::new);
        let rendered = renderer.render(candidates);
        self.attach_popup_buffer(rendered, qh)
    }

    fn attach_popup_buffer(
        &mut self,
        rendered: RenderedPopup,
        qh: &QueueHandle<Self>,
    ) -> Result<()> {
        if self.popup_buffers.len() >= 8 {
            warn!("candidate popup buffer queue is full; dropping this redraw");
            return Ok(());
        }
        let width = i32::try_from(rendered.width).context("popup width exceeds Wayland limits")?;
        let height =
            i32::try_from(rendered.height).context("popup height exceeds Wayland limits")?;
        let stride = width.checked_mul(4).context("popup row is too wide")?;
        let size = stride
            .checked_mul(height)
            .context("popup buffer is too large")?;
        let file = tempfile().context("create shared-memory popup buffer")?;
        file.set_len(size as u64)
            .context("size shared-memory popup buffer")?;
        // SAFETY: the backing file is sized before mapping and remains open while it is mapped.
        let mut mapping = unsafe { MmapOptions::new().len(size as usize).map_mut(&file) }
            .context("map shared-memory popup buffer")?;
        mapping.copy_from_slice(&rendered.pixels);
        let pool = self.shm.create_pool(file.as_fd(), size, qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        self.popup_surface.attach(Some(&buffer), 0, 0);
        self.popup_surface.damage(0, 0, width, height);
        self.popup_surface.commit();
        self.popup_buffers.push(ShmBuffer {
            proxy: buffer,
            _mapping: mapping,
        });
        Ok(())
    }

    fn forward_key(&mut self, time: u32, keycode: u32, pressed: bool) {
        let state = if pressed { 1 } else { 0 };
        self.virtual_keyboard.key(time, keycode, state);
    }

    fn fire_repeat(&mut self, qh: &QueueHandle<Self>) -> bool {
        let now = Instant::now();
        let interval = Duration::from_secs_f64(1.0 / f64::from(self.repeat_rate.max(1)));
        let Some(action) = self
            .repeat
            .as_mut()
            .and_then(|repeat| repeat.tick(now, interval))
        else {
            return false;
        };
        match action {
            RepeatAction::Forward { keycode, time } => self.forward_key(time, keycode, true),
            RepeatAction::Engine(event) => {
                let output = self.engine.handle_key(event);
                self.apply_output(output, qh);
            }
        }
        true
    }

    fn start_repeat(
        &mut self,
        keycode: u32,
        time: u32,
        event: Option<KeyEvent>,
        forwarded: bool,
        repeatable: bool,
    ) {
        if !repeatable || self.repeat_rate <= 0 {
            return;
        }
        let now = Instant::now();
        self.repeat = Some(Repeat {
            keycode,
            event,
            forwarded,
            time,
            last: now,
            next: now + Duration::from_millis(self.repeat_delay.max(0) as u64),
        });
    }

    fn key_event(&self, keycode: u32) -> Option<KeyEvent> {
        let keyboard = self.keyboard.as_ref()?;
        let xkb_keycode = xkb::Keycode::new(keycode.saturating_add(8));
        let symbol = keyboard.state.key_get_one_sym(xkb_keycode);
        let keysym = u32::from(symbol);
        let ctrl = keyboard
            .state
            .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let shift = keyboard
            .state
            .mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
        let alt = keyboard
            .state
            .mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let logo = keyboard
            .state
            .mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
        let key = if keysym == xkb::keysyms::KEY_Return || keysym == xkb::keysyms::KEY_KP_Enter {
            Key::Enter
        } else if keysym == xkb::keysyms::KEY_BackSpace {
            Key::Backspace
        } else if keysym == xkb::keysyms::KEY_Escape {
            Key::Escape
        } else if keysym == xkb::keysyms::KEY_Tab {
            Key::Tab
        } else if keysym == xkb::keysyms::KEY_space {
            Key::Space
        } else {
            let codepoint = xkb::keysym_to_utf32(symbol);
            let ch = char::from_u32(codepoint)?;
            if ctrl {
                Key::Ctrl(ch.to_ascii_lowercase())
            } else {
                Key::Char(ch)
            }
        };
        Some(KeyEvent {
            key,
            shift,
            ctrl,
            alt,
            logo,
        })
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wayland_client::protocol::wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _event: wayland_client::protocol::wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _event: wayland_client::protocol::wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _event: wayland_client::protocol::wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlShmPool,
        _event: wayland_client::protocol::wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlBuffer,
        event: wayland_client::protocol::wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wayland_client::protocol::wl_buffer::Event::Release) {
            state
                .popup_buffers
                .retain(|buffer| buffer.proxy.id() != proxy.id());
        }
    }
}

impl Dispatch<ZwpInputPopupSurfaceV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpInputPopupSurfaceV2,
        _event: PopupEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpInputMethodManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpInputMethodManagerV2,
        _: wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwpInputMethodV2,
        event: InputMethodEvent,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            InputMethodEvent::Activate => state.pending_active = Some(true),
            InputMethodEvent::Deactivate => state.pending_active = Some(false),
            InputMethodEvent::Done => {
                state.done_serial = state.done_serial.wrapping_add(1);
                if let Some(active) = state.pending_active.take() {
                    state.active = active;
                    if active {
                        state.engine.reset();
                        state.grab = Some(proxy.grab_keyboard(qh, ()));
                    } else {
                        state.repeat = None;
                        state.engine.reset();
                        if let Some(grab) = state.grab.take() {
                            grab.release();
                        }
                        if let Err(error) = state.update_popup(None, qh) {
                            warn!(%error, "could not clear candidate popup");
                        }
                    }
                }
            }
            InputMethodEvent::Unavailable => {
                state.unavailable = true;
                warn!(
                    "compositor reports the input method unavailable (another IME may own the seat)"
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpInputMethodKeyboardGrabV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodKeyboardGrabV2,
        event: GrabEvent,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            GrabEvent::Keymap { format, fd, size } => {
                if let Err(error) = install_keymap(state, format.into(), fd, size) {
                    warn!(%error, "could not install keyboard keymap");
                }
            }
            GrabEvent::Key {
                time,
                key,
                state: key_state,
                ..
            } => {
                let pressed = key_state == WEnum::Value(wl_keyboard::KeyState::Pressed);
                if !pressed {
                    if state.forwarded_keys.remove(&key) {
                        state.forward_key(time, key, false);
                    }
                    if state
                        .repeat
                        .as_ref()
                        .is_some_and(|repeat| repeat.keycode == key)
                    {
                        state.repeat = None;
                    }
                    return;
                }
                let repeatable = state.keyboard.as_ref().is_some_and(|keyboard| {
                    keyboard
                        .keymap
                        .key_repeats(xkb::Keycode::new(key.saturating_add(8)))
                });
                let Some(event) = state.key_event(key) else {
                    state.forward_key(time, key, true);
                    state.forwarded_keys.insert(key);
                    state.start_repeat(key, time, None, true, repeatable);
                    return;
                };
                let output = state.engine.handle_key(event.clone());
                let consumed = output.consumed;
                if consumed {
                    state.apply_output(output, qh);
                } else {
                    state.forward_key(time, key, true);
                    state.forwarded_keys.insert(key);
                }
                state.start_repeat(key, time, Some(event), !consumed, repeatable);
            }
            GrabEvent::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(keyboard) = &mut state.keyboard {
                    keyboard.state.update_mask(
                        mods_depressed,
                        mods_latched,
                        mods_locked,
                        0,
                        0,
                        group,
                    );
                }
                state
                    .virtual_keyboard
                    .modifiers(mods_depressed, mods_latched, mods_locked, group);
            }
            GrabEvent::RepeatInfo { rate, delay } => {
                state.repeat_rate = rate;
                state.repeat_delay = delay;
                if rate <= 0 {
                    state.repeat = None;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _event: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _event: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn install_keymap(state: &mut State, format: u32, fd: OwnedFd, size: u32) -> Result<()> {
    if format != KEYMAP_FORMAT_XKB_V1 {
        bail!("unsupported keymap format {format}");
    }
    let mut file = File::from(fd);
    let mut bytes = Vec::with_capacity(size as usize);
    file.read_to_end(&mut bytes)
        .context("read compositor keymap")?;
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    let keymap_text = String::from_utf8(bytes).context("keymap is not UTF-8")?;
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_string(
        &context,
        keymap_text,
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .context("compile compositor keymap")?;
    let keyboard_state = xkb::State::new(&keymap);
    let keyboard = KeyboardState {
        _context: context,
        keymap,
        state: keyboard_state,
    };
    file.seek(SeekFrom::Start(0))
        .context("rewind compositor keymap")?;
    state.virtual_keyboard.keymap(format, file.as_fd(), size);
    state.keyboard = Some(keyboard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
