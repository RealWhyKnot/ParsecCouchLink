# Changelog

All notable user-visible changes to Parsec CouchLink. The `Unreleased` section is appended from conventional commit subjects on `main`, then promoted to a tagged section by the release workflow.

## Unreleased

_No notable changes since the last release._

---

## [v2026.6.13.0](https://github.com/RealWhyKnot/ParsecCouchLink/releases/tag/v2026.6.13.0) -- 2026-06-14

### Added
- **cli:** Read configure-wifi credentials from environment variables (900d862)

### Fixed
- **cli:** Don't report a false join failure after configure-wifi (3f6f2aa)
- **cli:** Show Wi-Fi firmware as date.x when the build is unknown (e8e413d)
- **firmware:** Keep the XInput controller alive when Wi-Fi fails (ab5133d)
