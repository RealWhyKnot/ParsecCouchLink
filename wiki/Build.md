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
dist\ParsecCouchLink\couchlink-pico.uf2
dist\ParsecCouchLink\setup.ps1
```

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
.\scripts\build.ps1 -Release
```

## GitHub Workflows

- `ci.yml` checks the Rust bridge and builds the Pico firmware.
- `release.yml` builds the release zip on `v*` tags and publishes it.
- `wiki-sync.yml` mirrors `wiki/` to the GitHub Wiki.
