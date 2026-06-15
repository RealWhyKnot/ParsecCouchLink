# Changelog

All notable user-visible changes to Parsec CouchLink. The `Unreleased` section is appended from conventional commit subjects on `main`, then promoted to a tagged section by the release workflow.

## Unreleased

_No notable changes since the last release._

---

## [v2026.6.15.0-beta](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.15.0-beta) -- 2026-06-15

### Added
- Keyboard passthrough mode for keyboard-only games (bbdfc4c)

### Changed
- **firmware:** Clang-format keyboard persona helper (273d694)

### Fixed
- **build:** Keep version stamp out of worktree (da8de33)
- **protocol:** Harden full version discovery (e0a847b)
- **protocol:** Report full Wi-Fi firmware versions (3d884be)

---

## [v2026.6.14.1](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.14.1) -- 2026-06-15

### Fixed
- **firmware:** Keep provisioned picos in run mode (086e009)

---

## [v2026.6.14.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.14.0) -- 2026-06-15

_Maintenance release; see commit log for details._

---

## [v2026.6.13.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.13.0) -- 2026-06-14

### Added
- **cli:** Read configure-wifi credentials from environment variables (900d862)

### Fixed
- **cli:** Don't report a false join failure after configure-wifi (3f6f2aa)
- **cli:** Show Wi-Fi firmware as date.x when the build is unknown (e8e413d)
- **firmware:** Keep the XInput controller alive when Wi-Fi fails (ab5133d)
