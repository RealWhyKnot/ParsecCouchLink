//! Host keyboard capture for the HID keyboard persona.
//!
//! This captures **only the remote Parsec player's keystrokes, not the
//! host operator's**. The controller path gets this for free: it reads
//! Parsec's virtual XInput pad, a distinct device. A keyboard has no such
//! per-source split in Win32, so instead a low-level keyboard hook
//! (`WH_KEYBOARD_LL`) records only events Windows flags as injected
//! (`LLKHF_INJECTED`) -- the flag set on `SendInput`-style injection,
//! which is how Parsec replays the remote player's input, and never set
//! on physically-typed keys. Each stream tick snapshots that injected
//! key state into a USB HID boot-keyboard report.
//!
//! This holds as of current Parsec: its virtual USB driver emulates
//! gamepads, a mic, a presence-only mouse, and a tablet, but *no* virtual
//! keyboard, so guest typing can only arrive via OS-level injection
//! (which sets the injected flag). If a future Parsec build adds a virtual
//! HID keyboard *device*, its keys would not be flagged injected and
//! nothing would be captured -- the symptom is "typing does nothing", and
//! the one-time "capturing Parsec-injected input" log below never fires.
//! The migration path then is Raw Input (`RIDEV_INPUTSINK`), identifying
//! injected keys by a null/empty `hDevice` while real keyboards carry a
//! valid handle -- still excluding the host operator's physical typing.

use crate::protocol::KeyboardReport;

// HID Keyboard/Keypad usage IDs (page 0x07).
const HID_A: u8 = 0x04;
const HID_1: u8 = 0x1E;
const HID_0: u8 = 0x27;
const HID_F1: u8 = 0x3A;
const HID_KP_1: u8 = 0x59;
const HID_KP_0: u8 = 0x62;
const HID_ERR_ROLLOVER: u8 = 0x01;

// Modifier byte bit positions (HID boot report byte 0).
const MOD_LCTRL: u8 = 1 << 0;
const MOD_LSHIFT: u8 = 1 << 1;
const MOD_LALT: u8 = 1 << 2;
const MOD_LGUI: u8 = 1 << 3;
const MOD_RCTRL: u8 = 1 << 4;
const MOD_RSHIFT: u8 = 1 << 5;
const MOD_RALT: u8 = 1 << 6;
const MOD_RGUI: u8 = 1 << 7;

// Windows virtual-key codes used directly so the mapping never depends on
// which VK_* constants the `windows` crate happens to expose. These are
// the stable Win32 values documented by Microsoft.
const VK_BACK: i32 = 0x08;
const VK_TAB: i32 = 0x09;
const VK_RETURN: i32 = 0x0D;
const VK_PAUSE: i32 = 0x13;
const VK_CAPITAL: i32 = 0x14;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_PRIOR: i32 = 0x21; // Page Up
const VK_NEXT: i32 = 0x22; // Page Down
const VK_END: i32 = 0x23;
const VK_HOME: i32 = 0x24;
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
const VK_SNAPSHOT: i32 = 0x2C; // Print Screen
const VK_INSERT: i32 = 0x2D;
const VK_DELETE: i32 = 0x2E;
const VK_MULTIPLY: i32 = 0x6A;
const VK_ADD: i32 = 0x6B;
const VK_SUBTRACT: i32 = 0x6D;
const VK_DECIMAL: i32 = 0x6E;
const VK_DIVIDE: i32 = 0x6F;
const VK_NUMLOCK: i32 = 0x90;
const VK_SCROLL: i32 = 0x91;
const VK_OEM_1: i32 = 0xBA; // ; :
const VK_OEM_PLUS: i32 = 0xBB; // = +
const VK_OEM_COMMA: i32 = 0xBC; // , <
const VK_OEM_MINUS: i32 = 0xBD; // - _
const VK_OEM_PERIOD: i32 = 0xBE; // . >
const VK_OEM_2: i32 = 0xBF; // / ?
const VK_OEM_3: i32 = 0xC0; // ` ~
const VK_OEM_4: i32 = 0xDB; // [ {
const VK_OEM_5: i32 = 0xDC; // \ |
const VK_OEM_6: i32 = 0xDD; // ] }
const VK_OEM_7: i32 = 0xDE; // ' "

// Modifier virtual keys. Side-specific plus the generic variants: an
// injector may report either, so we check both.
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12; // generic Alt
const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;
const VK_LSHIFT: i32 = 0xA0;
const VK_RSHIFT: i32 = 0xA1;
const VK_LCONTROL: i32 = 0xA2;
const VK_RCONTROL: i32 = 0xA3;
const VK_LMENU: i32 = 0xA4; // left Alt
const VK_RMENU: i32 = 0xA5; // right Alt

// Misc named keys that aren't covered by the letter/digit/F-key/numpad
// ranges in `vk_to_hid`.
const NAMED_VK_HID: [(i32, u8); 35] = [
    (VK_RETURN, 0x28),
    (VK_ESCAPE, 0x29),
    (VK_BACK, 0x2A),
    (VK_TAB, 0x2B),
    (VK_SPACE, 0x2C),
    (VK_OEM_MINUS, 0x2D),
    (VK_OEM_PLUS, 0x2E),
    (VK_OEM_4, 0x2F),
    (VK_OEM_6, 0x30),
    (VK_OEM_5, 0x31),
    (VK_OEM_1, 0x33),
    (VK_OEM_7, 0x34),
    (VK_OEM_3, 0x35),
    (VK_OEM_COMMA, 0x36),
    (VK_OEM_PERIOD, 0x37),
    (VK_OEM_2, 0x38),
    (VK_CAPITAL, 0x39),
    (VK_SNAPSHOT, 0x46),
    (VK_SCROLL, 0x47),
    (VK_PAUSE, 0x48),
    (VK_INSERT, 0x49),
    (VK_HOME, 0x4A),
    (VK_PRIOR, 0x4B),
    (VK_DELETE, 0x4C),
    (VK_END, 0x4D),
    (VK_NEXT, 0x4E),
    (VK_RIGHT, 0x4F),
    (VK_LEFT, 0x50),
    (VK_DOWN, 0x51),
    (VK_UP, 0x52),
    (VK_NUMLOCK, 0x53),
    (VK_DIVIDE, 0x54),
    (VK_MULTIPLY, 0x55),
    (VK_SUBTRACT, 0x56),
    (VK_ADD, 0x57),
];

/// Map a Windows virtual-key code to a HID keyboard usage id, or `None`
/// if it isn't a key we forward (modifiers, mouse buttons, and unmapped
/// keys all return `None` -- modifiers are handled in the modifier byte).
fn vk_to_hid(vk: i32) -> Option<u8> {
    // Letters A-Z (VK 0x41..0x5A) -> HID 0x04..0x1D.
    if (0x41..=0x5A).contains(&vk) {
        return Some(HID_A + (vk - 0x41) as u8);
    }
    // Digit row 1-9 (VK 0x31..0x39) -> HID 0x1E..0x26, then 0 -> 0x27.
    if (0x31..=0x39).contains(&vk) {
        return Some(HID_1 + (vk - 0x31) as u8);
    }
    if vk == 0x30 {
        return Some(HID_0);
    }
    // F1-F12 (VK 0x70..0x7B) -> HID 0x3A..0x45.
    if (0x70..=0x7B).contains(&vk) {
        return Some(HID_F1 + (vk - 0x70) as u8);
    }
    // Numpad 1-9 (VK 0x61..0x69) -> HID 0x59..0x61, then numpad 0 -> 0x62.
    if (0x61..=0x69).contains(&vk) {
        return Some(HID_KP_1 + (vk - 0x61) as u8);
    }
    if vk == 0x60 {
        return Some(HID_KP_0);
    }
    if vk == VK_DECIMAL {
        return Some(0x63);
    }
    NAMED_VK_HID
        .iter()
        .find_map(|&(k, hid)| (k == vk).then_some(hid))
}

/// Build a boot-keyboard report from a modifier byte and the HID usage
/// ids of the currently-pressed keys. Enforces 6-key rollover: more than
/// six simultaneous keys yields the standard ErrorRollOver report (all
/// six slots = 0x01) so the host sees an explicit overflow rather than a
/// silently truncated chord.
fn build_report(modifiers: u8, mut pressed: Vec<u8>) -> KeyboardReport {
    pressed.sort_unstable();
    pressed.dedup();
    let mut keys = [0u8; 6];
    if pressed.len() > 6 {
        keys = [HID_ERR_ROLLOVER; 6];
    } else {
        for (slot, hid) in keys.iter_mut().zip(pressed) {
            *slot = hid;
        }
    }
    KeyboardReport { modifiers, keys }
}

/// Compose the modifier byte. Each bit checks the side-specific VK and,
/// for the three two-sided modifiers, the generic VK too -- an injector
/// that reports a generic modifier maps to the left bit, which is
/// functionally identical for typing.
fn modifier_byte(down: &impl Fn(i32) -> bool) -> u8 {
    let mut m = 0u8;
    if down(VK_LCONTROL) || down(VK_CONTROL) {
        m |= MOD_LCTRL;
    }
    if down(VK_RCONTROL) {
        m |= MOD_RCTRL;
    }
    if down(VK_LSHIFT) || down(VK_SHIFT) {
        m |= MOD_LSHIFT;
    }
    if down(VK_RSHIFT) {
        m |= MOD_RSHIFT;
    }
    if down(VK_LMENU) || down(VK_MENU) {
        m |= MOD_LALT;
    }
    if down(VK_RMENU) {
        m |= MOD_RALT;
    }
    if down(VK_LWIN) {
        m |= MOD_LGUI;
    }
    if down(VK_RWIN) {
        m |= MOD_RGUI;
    }
    m
}

/// Core capture, parameterised on a key-state probe so it can be unit
/// tested without Win32. Scans every virtual-key code we know how to map
/// plus the modifier keys.
fn read_keyboard_with(down: impl Fn(i32) -> bool) -> KeyboardReport {
    let modifiers = modifier_byte(&down);
    let mut pressed = Vec::new();
    for vk in 0x08..=0xFEi32 {
        if let Some(hid) = vk_to_hid(vk) {
            if down(vk) {
                pressed.push(hid);
            }
        }
    }
    build_report(modifiers, pressed)
}

/// Snapshot the host keyboard into a HID boot-keyboard report, reading
/// only the Parsec-injected key state collected by the low-level hook.
#[cfg(windows)]
pub fn read_keyboard() -> KeyboardReport {
    capture::ensure_started();
    read_keyboard_with(capture::is_injected_down)
}

/// Install the injected-input hook ahead of streaming so the first
/// reports aren't empty while the hook thread spins up. Idempotent.
#[cfg(windows)]
pub fn start_capture() {
    capture::ensure_started();
}

#[cfg(windows)]
mod capture {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Once;

    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, HC_ACTION, HHOOK,
        KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    // 256 virtual-key codes -> 4 x u64 bitmap of "currently held via
    // injected (Parsec) input". Written only by the hook thread, read from
    // the streaming thread; relaxed ordering is fine since each key is an
    // independent boolean and a one-tick skew is invisible.
    static INJECTED: [AtomicU64; 4] = [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ];
    static STARTED: Once = Once::new();
    // Latched the first time we see an injected key, to log a single
    // confirmation that the hook is capturing Parsec input.
    static SEEN_INJECTED: AtomicBool = AtomicBool::new(false);

    fn set_key(vk: u32, down: bool) {
        if vk > 255 {
            return;
        }
        let word = &INJECTED[(vk / 64) as usize];
        let bit = 1u64 << (vk % 64);
        if down {
            word.fetch_or(bit, Ordering::Relaxed);
        } else {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    pub fn is_injected_down(vk: i32) -> bool {
        if !(0..=255).contains(&vk) {
            return false;
        }
        let word = INJECTED[(vk / 64) as usize].load(Ordering::Relaxed);
        word & (1u64 << (vk % 64)) != 0
    }

    unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Mirror only events Windows marks injected (SendInput), i.e.
            // Parsec's replayed remote input. Physical keys typed at the
            // host carry no such flag and are ignored.
            if kbd.flags.0 & LLKHF_INJECTED.0 != 0 {
                let down = match wparam.0 as u32 {
                    WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
                    WM_KEYUP | WM_SYSKEYUP => Some(false),
                    _ => None,
                };
                if let Some(is_down) = down {
                    set_key(kbd.vkCode, is_down);
                    // One-time proof the hook is actually seeing Parsec
                    // input. If a guest is typing and this never logs, Parsec
                    // changed how it injects keyboard input (see module docs).
                    if is_down && !SEEN_INJECTED.swap(true, Ordering::Relaxed) {
                        tracing::info!(
                            "keyboard: capturing Parsec-injected input (first key vk=0x{:02X} sc=0x{:02X})",
                            kbd.vkCode,
                            kbd.scanCode
                        );
                    }
                    // Per-key detail for empirical validation under -vv: shows
                    // whether modifiers arrive as generic or side-specific VKs.
                    tracing::trace!(
                        "keyboard: injected vk=0x{:02X} sc=0x{:02X} {} ext={}",
                        kbd.vkCode,
                        kbd.scanCode,
                        if is_down { "down" } else { "up" },
                        (kbd.flags.0 & LLKHF_EXTENDED.0) != 0
                    );
                }
            }
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    fn hook_thread() {
        unsafe {
            let hmod = GetModuleHandleW(None).unwrap_or_default();
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), HINSTANCE(hmod.0), 0) {
                Ok(_) => {
                    // Low-level hook callbacks are delivered on this thread
                    // while it pumps messages; block here for the process
                    // lifetime. GetMessageW never returns >0 in practice
                    // (no window, no posted messages), but the pump is what
                    // lets the OS run the hook proc.
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        DispatchMessageW(&msg);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "keyboard: could not install injected-input hook ({e:#}); \
                         no keyboard input will be captured"
                    );
                }
            }
        }
    }

    pub fn ensure_started() {
        STARTED.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("couchlink-kbd-hook".into())
                .spawn(hook_thread);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_hid() {
        assert_eq!(vk_to_hid(0x41), Some(0x04)); // A
        assert_eq!(vk_to_hid(0x5A), Some(0x1D)); // Z
    }

    #[test]
    fn digits_map_to_hid() {
        assert_eq!(vk_to_hid(0x31), Some(0x1E)); // 1
        assert_eq!(vk_to_hid(0x39), Some(0x26)); // 9
        assert_eq!(vk_to_hid(0x30), Some(0x27)); // 0
    }

    #[test]
    fn named_and_function_keys_map() {
        assert_eq!(vk_to_hid(VK_RETURN), Some(0x28));
        assert_eq!(vk_to_hid(VK_SPACE), Some(0x2C));
        assert_eq!(vk_to_hid(VK_BACK), Some(0x2A));
        assert_eq!(vk_to_hid(VK_ESCAPE), Some(0x29));
        assert_eq!(vk_to_hid(0x70), Some(0x3A)); // F1
        assert_eq!(vk_to_hid(0x7B), Some(0x45)); // F12
        assert_eq!(vk_to_hid(VK_LEFT), Some(0x50));
    }

    #[test]
    fn numpad_keys_map() {
        assert_eq!(vk_to_hid(0x61), Some(0x59)); // numpad 1
        assert_eq!(vk_to_hid(0x60), Some(0x62)); // numpad 0
        assert_eq!(vk_to_hid(VK_DECIMAL), Some(0x63));
        assert_eq!(vk_to_hid(VK_DIVIDE), Some(0x54));
    }

    #[test]
    fn modifiers_and_mouse_are_not_keys() {
        // Modifier VKs are handled in the modifier byte, not the key array.
        assert_eq!(vk_to_hid(VK_LSHIFT), None);
        assert_eq!(vk_to_hid(VK_LCONTROL), None);
        assert_eq!(vk_to_hid(VK_LWIN), None);
        // Left/right mouse buttons.
        assert_eq!(vk_to_hid(0x01), None);
        assert_eq!(vk_to_hid(0x02), None);
    }

    #[test]
    fn captures_modifiers_and_keys() {
        // Hold Left Shift + A.
        let down = |vk: i32| vk == VK_LSHIFT || vk == 0x41;
        let rep = read_keyboard_with(down);
        assert_eq!(rep.modifiers, MOD_LSHIFT);
        assert_eq!(rep.keys, [0x04, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn captures_multiple_modifiers_with_a_key() {
        // Ctrl+Alt+Delete: left ctrl + left alt + Delete, no key letters.
        let down = |vk: i32| vk == VK_LCONTROL || vk == VK_LMENU || vk == VK_DELETE;
        let rep = read_keyboard_with(down);
        assert_eq!(rep.modifiers, MOD_LCTRL | MOD_LALT);
        assert_eq!(rep.keys, [0x4C, 0, 0, 0, 0, 0]); // Delete = HID 0x4C
    }

    #[test]
    fn no_keys_down_is_neutral() {
        let rep = read_keyboard_with(|_| false);
        assert_eq!(rep.modifiers, 0);
        assert_eq!(rep.keys, [0u8; 6]);
    }

    #[test]
    fn generic_modifier_vks_map_to_left_bits() {
        // An injector that reports the generic Shift/Ctrl/Alt VK (rather
        // than the side-specific one) must still set the modifier, mapped
        // to the left bit.
        let rep = read_keyboard_with(|vk| vk == VK_SHIFT);
        assert_eq!(rep.modifiers, MOD_LSHIFT);
        let rep = read_keyboard_with(|vk| vk == VK_CONTROL || vk == VK_MENU);
        assert_eq!(rep.modifiers, MOD_LCTRL | MOD_LALT);
        // Right-side specific stays distinct from the generic mapping.
        let rep = read_keyboard_with(|vk| vk == VK_RSHIFT);
        assert_eq!(rep.modifiers, MOD_RSHIFT);
    }

    #[test]
    fn six_key_rollover_truncates_to_error() {
        // Seven letter keys down at once -> ErrorRollOver.
        let held = [0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        let down = move |vk: i32| held.contains(&vk);
        let rep = read_keyboard_with(down);
        assert_eq!(rep.keys, [HID_ERR_ROLLOVER; 6]);
    }

    #[test]
    fn exactly_six_keys_fit() {
        let held = [0x41, 0x42, 0x43, 0x44, 0x45, 0x46];
        let down = move |vk: i32| held.contains(&vk);
        let rep = read_keyboard_with(down);
        assert_eq!(rep.keys, [0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);
    }
}
