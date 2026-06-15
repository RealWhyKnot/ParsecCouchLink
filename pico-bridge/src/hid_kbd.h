#pragma once

// Pump one HID boot-keyboard IN report onto the USB endpoint when the
// host is ready for one. Cheap to call every iteration of the run-mode
// main loop; cooperates with TinyUSB's internal scheduling. Only used
// when the active run persona is RUN_PERSONA_KEYBOARD.
void hid_kbd_init(void);
void hid_kbd_task(void);
void hid_kbd_note_usb_reset(void);
