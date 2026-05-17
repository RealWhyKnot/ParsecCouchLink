# Reporting bugs

If something didn't work, the fastest way to get it fixed is to attach a bundle to the issue. The bundle is one command and it captures everything -- logs, doctor output, firmware state -- in a single ZIP with no manual log-hunting required.

## Make a bundle

From the folder where you extracted the release:

```powershell
.\couchlink.exe bundle
```

This writes a timestamped ZIP next to `couchlink.exe`, named something like `couchlink-bundle-20260516-143012.zip`.

The ZIP contains:

- `manifest.json` -- versions and timestamps
- `doctor.txt` -- full output of `couchlink.exe doctor`
- Recent bridge log files
- Setup transcripts from `%LOCALAPPDATA%\ParsecCouchLink\data\logs\`
- Crash files, if any
- `pico-diag.txt` -- firmware diagnostics, if the Pico is currently in setup mode

The bundle does NOT contain your Wi-Fi password or SSID.

## What's in the bundle

- `manifest.json` - versions, timestamps, build info
- `doctor.txt` - diagnostic check output
- Bridge log files (recent)
- Setup transcripts
- Crash files (if present)
- `pico-diag.txt` (if Pico is in USB setup mode at time of bundle)

## Where to send it

Open a new issue at:

https://github.com/RealWhyKnot/ParsecCouchLink/issues

Fill in the form fields, then drag the ZIP into the comment box at the bottom of the page before submitting.

## What if couchlink won't run at all?

If `couchlink.exe` doesn't start, the bundle command won't work. In that case:

1. Grab the setup transcript at `%LOCALAPPDATA%\ParsecCouchLink\data\logs\setup-*.log` and attach it to the issue.
2. Include a screenshot of the terminal showing the error.

If `couchlink.exe` starts but fails at the doctor stage, paste the doctor output as text in the issue body -- it's short and easier to read inline than as an attachment.
