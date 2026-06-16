# Changelog

All notable user-visible changes to Parsec CouchLink. The `Unreleased` section is appended from conventional commit subjects on `main`, then promoted to a tagged section by the release workflow.

## Unreleased

_No notable changes since the last release._

---

## [v2026.6.16.1-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.16.1-beta) -- 2026-06-16

### Added
- **bundle:** Fold debug capture into support bundle (2bcdcd9)
- **diagnostics:** Capture first usb traffic events (fd8c04f)
- **diagnostics:** Capture usb lifecycle events (2060511)
- **bundle:** Classify usb descriptor identity (4016ad1)
- **bundle:** Add usb enumeration analysis (6acaf9c)
- **bundle:** Summarize usb setup behavior (18b25d4)
- **diagnostics:** Report debug packet capture loss (f239031)
- **diagnostics:** Record usb input block reasons (c08ed35)
- **diagnostics:** Add debug capture evidence report (4c20885)
- **diagnostics:** Add usb packet timeline (e987395)
- **diagnostics:** Summarize hid report packets (b8cc004)
- **diagnostics:** Log hid control debug packets (7dca6cb)
- **diagnostics:** Harvest debug packets during bundle (da9e0a4)
- **diagnostics:** Record debug harvest health (4cb55db)
- **diagnostics:** Gate debug packet evidence (ce7f025)
- **diagnostics:** Decode bundled usb control traffic (ae9833d)
- **diagnostics:** Add usb control transfer transcript (9ea06e8)
- **diagnostics:** Include harvest evidence in packet dump (75c1cb6)
- **diagnostics:** Decode debug usb setup packets (f526282)
- **diagnostics:** Explain debug packet capture verdict (83f7eab)
- **diagnostics:** Record debug packet harvest health (eab01a6)
- **diagnostics:** Emit structured usb packet records (87d075a)
- **diagnostics:** Summarize bundled usb packets (f9abf0e)
- **diagnostics:** Summarize debug usb packet capture (6065d90)
- **diagnostics:** Capture debug usb control traffic (7a08b39)
- **diagnostics:** Harvest debug packets without blocking stream (7f41d83)
- **diagnostics:** Capture debug input reports (ac2f141)
- **diagnostics:** Retain debug packet captures (1571e00)
- **diagnostics:** Add debug input packet capture (02e6d33)
- **diagnostics:** Expand bundle capture (122ce6c)

### Fixed
- **bundle:** Redact private support bundle details (8040fc1)
- **doctor:** Use multi-interface pico discovery (09fd3cc)

---

## [v2026.6.15.2-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.15.2-beta) -- 2026-06-15

### Added
- **persona:** Add adaptive gamepad modes (3abdef6)

### Fixed
- **release:** Use central date for prereleases (c85ed03)

---

## [v2026.6.15.1-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.15.1-beta) -- 2026-06-15

### Added
- **dinput:** Add 8BitDo Pro 2 persona (64c5d2e)
- **maple:** Add Dreamcast output persona (afcbf01)

### Changed
- **maple:** Restore experimental persona selection (f6455b5)

### Fixed
- **maple:** Use Xbox-compatible adapter mode (f072050)

---

## [v2026.6.15.0-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.15.0-beta) -- 2026-06-15

### Added
- Keyboard passthrough mode for keyboard-only games (bbdfc4c)

### Changed
- **firmware:** Clang-format keyboard persona helper (273d694)

### Fixed
- **protocol:** Harden full version discovery (e0a847b)
- **protocol:** Report full Wi-Fi firmware versions (3d884be)
- **build:** Keep version stamp out of worktree (da8de33)

---

## [v2026.6.14.1](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.14.1) -- 2026-06-15

### Fixed
- **firmware:** Keep provisioned picos in run mode (086e009)

---

## [v2026.6.14.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.14.0) -- 2026-06-15

### Added
- **lab:** Add PnP reconnect bench support (e946cb8)

### Fixed
- **lab:** Handle directed discovery and post-flash run mode (ceb6336)

---

## [v2026.6.13.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.13.0) -- 2026-06-14

### Added
- **ui:** Add device-first terminal tabs (5ffc6aa)
- **cli:** Add hardware lab harness (41bf9e7)
- **cli:** Read configure-wifi credentials from environment variables (900d862)

### Fixed
- **cli:** Don't report a false join failure after configure-wifi (3f6f2aa)
- **cli:** Show Wi-Fi firmware as date.x when the build is unknown (e8e413d)
- **firmware:** Keep the XInput controller alive when Wi-Fi fails (ab5133d)

---

## [v2026.5.31.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.31.0) -- 2026-05-31

### Added
- **cli:** Add saved Pico home view (9b536ea)
- **cli:** Auto-recover setup-mode picos (d48a65f)
- **cli:** Add tools diagnostics hub (8c1783d)
- **cli:** Improve guided menu help (b40c9d3)
- **cli:** Add quick BOOTSEL command (c01c43f)
- **debug:** Add Pico mode recovery menu (ba8abf8)
- **cli:** Add Pico USB adapter diagnostics (6c36268)
- **cli:** Support manual Pico IP discovery (7190756)

### Changed
- **firmware:** Poll the XInput IN endpoint at 1 ms (bf98c83)
- **cli:** Finish moving the menus onto the shared TUI module (9b7afed)
- **cli:** Share one TUI prompt module across the menus (b690b57)
- **bundle:** Split cmd_bundle into focused submodules (016620a)
- **bridge:** Drop the unused network module (89b6c1c)

### Fixed
- **cdc:** Name the opcode in "unexpected response" errors (b1ebf93)
- **cli:** Explain when a saved layout falls back to auto-detect (35cc4c2)
- **stream:** Keep streaming when a Pico briefly drops (a8c3606)
- **firmware:** Improve Pico recovery paths (e542d11)
- **cli:** Simplify guided firmware update (dd32e65)
- **cli:** Add Wi-Fi discovery recovery steps (36dcf6b)
- **setup:** Remove controller prompt from first-run setup (738f367)

---

## [v2026.5.30.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.30.0) -- 2026-05-30

### Added
- **cli:** Add guided controller routing (e1f72d8)
- **flash:** Support USB-only dual-board reflashing (8698ea9)

### Fixed
- **cli:** Harden routing recovery (7b6b416)

---

## [v2026.5.22.0-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.22.0-beta) -- 2026-05-22

### Added
- Force setup mode after UF2 reflash; parent-only bundle diag (eb54c5b)
- **lab:** Identify, discover, ping, log-tail, sleep commands (69be41f)
- **lab:** DPAPI wifi vault + save-wifi CLI + wifi_apply_saved (8b4878f)
- **bridge:** Replace remote sessions with lab-mode (2dbbbba)

### Changed
- **release:** Drop lab-mode leftovers from CouchLink (4d62946)
- **bridge/network:** Drop trailing blank line for cargo fmt (76b996c)
- **firmware:** Drop lab-mode UDP + CDC bootsel paths (af7c430)
- **bridge:** Remove remote lab-mode subsystem (bc12331)
- **lab:** Tighten upload chunking, audit framing, send semantics (db03d0f)

### Fixed
- **firmware:** Pump tud_task between tusb_init and mode dispatch (159ad21)

---

## [v2026.5.20.1-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.20.1-beta) -- 2026-05-21

### Added
- **lab:** Identify, discover, ping, log-tail, sleep commands (e88b13d)
- **lab:** DPAPI wifi vault + save-wifi CLI + wifi_apply_saved (4689586)
- **bridge:** Replace remote sessions with lab-mode (2c4c519)
- **bridge:** Reboot-to-bootsel CDC + UDP helpers (e654c02)
- **firmware:** Reboot-to-bootsel on CDC and UDP (926e827)

### Changed
- **lab:** Tighten upload chunking, audit framing, send semantics (a590c82)
- **bridge:** Factor flash/bundle/doctor cores out of CLI (e26e089)

### Fixed
- **firmware:** Pump tud_task between tusb_init and mode dispatch (8ee866a)

---

## [v2026.5.20.0-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.20.0-beta) -- 2026-05-20

### Added
- **bridge:** Remote-debug tunnel via WSS (0703a08)

### Changed
- **bridge:** Rustfmt diag_usb enum variants (b613a42)

### Fixed
- RP2040 USB enumeration on first boot + run-mode Wi-Fi diag gap (b6912c5)

---

## [v2026.5.18.1-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.18.1-beta) -- 2026-05-19

### Added
- **usb:** Defer D+ pull-up; add WinUSB diag channel (c7c65a9)

---

## [v2026.5.18.0-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.18.0-beta) -- 2026-05-19

### Added
- Heartbeat, fault context, state journal, USB event log, self-narrating stubs (94f8b6b)
- **bundle:** Structured diag capture + UDP log channel; restore bridge logging (82f7bad)

---

## [v2026.5.16.5-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.5-beta) -- 2026-05-18

### Added
- Robustness, diagnostics, and silent-failure pass (v2026.5.16.5) (7f03b9f)

### Fixed
- **build:** Align version regex with release.yml for -beta tags (250d54a)

---

## [v2026.5.16.4](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.4) -- 2026-05-17

### Fixed
- **bundle:** Match tracing-appender's filename format for bridge logs (26d55b0)

---

## [v2026.5.16.3](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.3) -- 2026-05-17

### Added
- **scripts:** One-shot PowerShell wrapper per couchlink subcommand (ba883bb)

### Changed
- **bridge:** Rustfmt the setup walkthrough call (8d50534)

### Fixed
- **cdc:** Assert DTR + accept RX without DTR; unify log paths (2e4d4ed)

---

## [v2026.5.16.2](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.2) -- 2026-05-17

### Fixed
- **firmware:** Enumerate USB during the BOOTSEL recovery window (53a4b9a)

---

## [v2026.5.16.1](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.1) -- 2026-05-16

### Added
- **setup:** Harden first-run wizard against silent failures (667db9a)

---

## [v2026.5.16.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.16.0) -- 2026-05-16

### Added
- Self-describing failure paths and richer bug reports (531d74d)

---

## [v2026.5.15.2](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.15.2) -- 2026-05-15

### Added
- Native Pico W (RP2040) support alongside Pico 2 W (1844f00)

---

## [v2026.5.15.1](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.5.15.1) -- 2026-05-15

### Added
- Initial public release package with the Windows bridge, Pico firmware, setup script, and release manifest.

---
