# Build

Local builds use the root `build.ps1`, which stages the same shape as a release zip.

## Requirements

- Rust stable.
- CMake.
- Ninja.
- ARM GNU Toolchain with `arm-none-eabi-gcc` on `PATH`.
- PowerShell 7 or Windows PowerShell.

## Build Everything

```powershell
.\build.ps1
```

Output:

```text
dist\ParsecCouchLink\couchlink.exe
dist\ParsecCouchLink\couchlink-pico2w.uf2
dist\ParsecCouchLink\couchlink-picow.uf2
dist\ParsecCouchLink\setup.ps1
```

Both Pico variants are built every time. The release setup script picks the matching firmware at flash time based on which Pico is in BOOTSEL, so only one of the two UF2s is actually written to a given device.

## Build A Release Zip

```powershell
.\build.ps1 -Package
```

Output:

```text
dist\ParsecCouchLink-v<version>.zip
dist\ParsecCouchLink-v<version>.manifest.tsv
```

The manifest lists each file in the zip with size and SHA-256.

## Build Only One Side

Rebuild just the Windows bridge:

```powershell
.\build.ps1 -SkipPico
```

Rebuild just the Pico firmware after a bridge build already exists:

```powershell
.\build.ps1 -SkipBridge
```

The Pico helper can also be run directly:

```powershell
cd pico-bridge
.\scripts\build.ps1 -Release            # both boards
.\scripts\build.ps1 -Release -Board pico2_w   # Pico 2 W only
.\scripts\build.ps1 -Release -Board pico_w    # Pico W / WH only
```

Outputs land in `pico-bridge\dist\`:

```text
pico-bridge\dist\couchlink-pico2w.uf2
pico-bridge\dist\couchlink-picow.uf2
```

The pico-sdk clone is shared across per-board build directories (`build-pico2w/`, `build-picow/`) via a sibling `build-_pico_sdk/` folder, so a dual-board build only fetches the SDK once.

## Tests

Run the Rust bridge tests:

```powershell
cargo test --manifest-path bridge\Cargo.toml
```

Run the C host tests for firmware setup and boot-mode policy code:

```powershell
cmake -S pico-bridge -B build-host-tests -DPICO_BRIDGE_HOST_TESTS=ON
cmake --build build-host-tests --config Debug
ctest --test-dir build-host-tests -C Debug --output-on-failure
```

Run the formatting and lint wrapper used locally before commits:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
```

For plugged-in Pico benches, see [[Hardware Lab]]. The lab harness can produce
a JSON report and can exercise setup-mode reconnects, BOOTSEL flash cycles,
run-mode XInput enumeration, and controller signal checks.

## GitHub Workflows

- `ci.yml` checks the Rust bridge and builds the Pico firmware.
- `release.yml` builds the release zip on `v*` tags and publishes it.
- `wiki-sync.yml` mirrors `wiki/` to the GitHub Wiki.
