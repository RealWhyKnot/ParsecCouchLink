use crate::protocol::KeyboardReport;

const HID_A: u8 = 0x04;
const HID_1: u8 = 0x1E;
const HID_0: u8 = 0x27;
const HID_F1: u8 = 0x3A;
const HID_KP_1: u8 = 0x59;
const HID_KP_0: u8 = 0x62;
const HID_ERR_ROLLOVER: u8 = 0x01;

const MOD_LCTRL: u8 = 1 << 0;
const MOD_LSHIFT: u8 = 1 << 1;
const MOD_LALT: u8 = 1 << 2;
const MOD_LGUI: u8 = 1 << 3;
const MOD_RCTRL: u8 = 1 << 4;
const MOD_RSHIFT: u8 = 1 << 5;
const MOD_RALT: u8 = 1 << 6;
const MOD_RGUI: u8 = 1 << 7;

const VK_BACK: i32 = 0x08;
const VK_TAB: i32 = 0x09;
const VK_RETURN: i32 = 0x0D;
const VK_PAUSE: i32 = 0x13;
const VK_CAPITAL: i32 = 0x14;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_PRIOR: i32 = 0x21;
const VK_NEXT: i32 = 0x22;
const VK_END: i32 = 0x23;
const VK_HOME: i32 = 0x24;
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
const VK_SNAPSHOT: i32 = 0x2C;
const VK_INSERT: i32 = 0x2D;
const VK_DELETE: i32 = 0x2E;
const VK_MULTIPLY: i32 = 0x6A;
const VK_ADD: i32 = 0x6B;
const VK_SUBTRACT: i32 = 0x6D;
const VK_DECIMAL: i32 = 0x6E;
const VK_DIVIDE: i32 = 0x6F;
const VK_NUMLOCK: i32 = 0x90;
const VK_SCROLL: i32 = 0x91;
const VK_OEM_1: i32 = 0xBA;
const VK_OEM_PLUS: i32 = 0xBB;
const VK_OEM_COMMA: i32 = 0xBC;
const VK_OEM_MINUS: i32 = 0xBD;
const VK_OEM_PERIOD: i32 = 0xBE;
const VK_OEM_2: i32 = 0xBF;
const VK_OEM_3: i32 = 0xC0;
const VK_OEM_4: i32 = 0xDB;
const VK_OEM_5: i32 = 0xDC;
const VK_OEM_6: i32 = 0xDD;
const VK_OEM_7: i32 = 0xDE;

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;
const VK_LSHIFT: i32 = 0xA0;
const VK_RSHIFT: i32 = 0xA1;
const VK_LCONTROL: i32 = 0xA2;
const VK_RCONTROL: i32 = 0xA3;
const VK_LMENU: i32 = 0xA4;
const VK_RMENU: i32 = 0xA5;

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

fn vk_to_hid(vk: i32) -> Option<u8> {
    if (0x41..=0x5A).contains(&vk) {
        return u8::try_from(vk - 0x41).ok().map(|offset| HID_A + offset);
    }
    if (0x31..=0x39).contains(&vk) {
        return u8::try_from(vk - 0x31).ok().map(|offset| HID_1 + offset);
    }
    if vk == 0x30 {
        return Some(HID_0);
    }
    if (0x70..=0x7B).contains(&vk) {
        return u8::try_from(vk - 0x70).ok().map(|offset| HID_F1 + offset);
    }
    if (0x61..=0x69).contains(&vk) {
        return u8::try_from(vk - 0x61).ok().map(|offset| HID_KP_1 + offset);
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

fn read_keyboard_with(down: impl Fn(i32) -> bool) -> KeyboardReport {
    let modifiers = modifier_byte(&down);
    let mut pressed = Vec::new();
    for vk in 0x08..=0xFEi32 {
        if let Some(hid) = vk_to_hid(vk)
            && down(vk)
        {
            pressed.push(hid);
        }
    }
    build_report(modifiers, pressed)
}

#[cfg(windows)]
pub(crate) fn read_keyboard() -> KeyboardReport {
    capture::ensure_started();
    read_keyboard_with(capture::is_injected_down)
}

#[cfg(windows)]
pub(crate) fn start_capture() {
    capture::ensure_started();
}

#[cfg(windows)]
mod capture {
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        LLKHF_INJECTED, MSG, SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    static INJECTED: [AtomicU64; 4] = [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ];
    static STARTED: Once = Once::new();
    static SEEN_INJECTED: AtomicBool = AtomicBool::new(false);

    fn set_key(vk: u32, down: bool) {
        if vk > 255 {
            return;
        }
        let word = &INJECTED[crate::convert::usize_from_u32(vk / 64)];
        let bit = 1u64 << (vk % 64);
        if down {
            word.fetch_or(bit, Ordering::Relaxed);
        } else {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }

    pub(super) fn is_injected_down(vk: i32) -> bool {
        let Ok(index) = usize::try_from(vk) else {
            return false;
        };
        if index > 255 {
            return false;
        }
        let word = INJECTED[index / 64].load(Ordering::Relaxed);
        word & (1u64 << (index % 64)) != 0
    }

    unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: for HC_ACTION Windows passes a valid KBDLLHOOKSTRUCT pointer in `lparam`.
        unsafe {
            if u32::try_from(code) == Ok(HC_ACTION) {
                let kbd = &*std::ptr::with_exposed_provenance::<KBDLLHOOKSTRUCT>(
                    lparam.0.cast_unsigned(),
                );
                if kbd.flags.0 & LLKHF_INJECTED.0 != 0 {
                    let down = match u32::try_from(wparam.0) {
                        Ok(WM_KEYDOWN | WM_SYSKEYDOWN) => Some(true),
                        Ok(WM_KEYUP | WM_SYSKEYUP) => Some(false),
                        _ => None,
                    };
                    if let Some(is_down) = down {
                        set_key(kbd.vkCode, is_down);
                        if is_down && !SEEN_INJECTED.swap(true, Ordering::Relaxed) {
                            tracing::info!(
                                "keyboard: capturing Parsec-injected input (first key vk=0x{:02X} sc=0x{:02X})",
                                kbd.vkCode,
                                kbd.scanCode
                            );
                        }
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
            CallNextHookEx(None, code, wparam, lparam)
        }
    }

    fn hook_thread() {
        // SAFETY: `ll_proc` has the hook signature and `msg` is a live MSG for the pump.
        unsafe {
            let hmod = GetModuleHandleW(None).unwrap_or_default();
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), Some(HINSTANCE(hmod.0)), 0) {
                Ok(_) => {
                    let mut msg = MSG::default();
                    while GetMessageW(&raw mut msg, None, 0, 0).0 > 0 {
                        DispatchMessageW(&raw const msg);
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

    pub(super) fn ensure_started() {
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
        assert_eq!(vk_to_hid(0x41), Some(0x04));
        assert_eq!(vk_to_hid(0x5A), Some(0x1D));
    }

    #[test]
    fn digits_map_to_hid() {
        assert_eq!(vk_to_hid(0x31), Some(0x1E));
        assert_eq!(vk_to_hid(0x39), Some(0x26));
        assert_eq!(vk_to_hid(0x30), Some(0x27));
    }

    #[test]
    fn named_and_function_keys_map() {
        assert_eq!(vk_to_hid(VK_RETURN), Some(0x28));
        assert_eq!(vk_to_hid(VK_SPACE), Some(0x2C));
        assert_eq!(vk_to_hid(VK_BACK), Some(0x2A));
        assert_eq!(vk_to_hid(VK_ESCAPE), Some(0x29));
        assert_eq!(vk_to_hid(0x70), Some(0x3A));
        assert_eq!(vk_to_hid(0x7B), Some(0x45));
        assert_eq!(vk_to_hid(VK_LEFT), Some(0x50));
    }

    #[test]
    fn numpad_keys_map() {
        assert_eq!(vk_to_hid(0x61), Some(0x59));
        assert_eq!(vk_to_hid(0x60), Some(0x62));
        assert_eq!(vk_to_hid(VK_DECIMAL), Some(0x63));
        assert_eq!(vk_to_hid(VK_DIVIDE), Some(0x54));
    }

    #[test]
    fn modifiers_and_mouse_are_not_keys() {
        assert_eq!(vk_to_hid(VK_LSHIFT), None);
        assert_eq!(vk_to_hid(VK_LCONTROL), None);
        assert_eq!(vk_to_hid(VK_LWIN), None);
        assert_eq!(vk_to_hid(0x01), None);
        assert_eq!(vk_to_hid(0x02), None);
    }

    #[test]
    fn captures_modifiers_and_keys() {
        let down = |vk: i32| vk == VK_LSHIFT || vk == 0x41;
        let rep = read_keyboard_with(down);
        assert_eq!(rep.modifiers, MOD_LSHIFT);
        assert_eq!(rep.keys, [0x04, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn captures_multiple_modifiers_with_a_key() {
        let down = |vk: i32| vk == VK_LCONTROL || vk == VK_LMENU || vk == VK_DELETE;
        let rep = read_keyboard_with(down);
        assert_eq!(rep.modifiers, MOD_LCTRL | MOD_LALT);
        assert_eq!(rep.keys, [0x4C, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn no_keys_down_is_neutral() {
        let rep = read_keyboard_with(|_| false);
        assert_eq!(rep.modifiers, 0);
        assert_eq!(rep.keys, [0u8; 6]);
    }

    #[test]
    fn generic_modifier_vks_map_to_left_bits() {
        let rep = read_keyboard_with(|vk| vk == VK_SHIFT);
        assert_eq!(rep.modifiers, MOD_LSHIFT);
        let rep = read_keyboard_with(|vk| vk == VK_CONTROL || vk == VK_MENU);
        assert_eq!(rep.modifiers, MOD_LCTRL | MOD_LALT);
        let rep = read_keyboard_with(|vk| vk == VK_RSHIFT);
        assert_eq!(rep.modifiers, MOD_RSHIFT);
    }

    #[test]
    fn six_key_rollover_truncates_to_error() {
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
