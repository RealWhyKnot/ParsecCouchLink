# Lab Mode

Lab mode is an opt-in remote-flash session. The host starts a single
subcommand, gets back a short URL, and shares it with the operator
who is iterating on firmware from a different machine. The operator
uploads UF2 builds, forces the Pico into BOOTSEL drive mode, flashes,
runs the bridge's diagnostic ladder, and pulls the firmware diag ring
-- all over a tunnel, with no shell or arbitrary file-read surface.

This page is for hosts (people who have the Pico physically plugged
in) and operators (developers who want remote access for testing).

---

## Trust model up front

* The host runs `couchlink lab-mode` intentionally. The bridge is a
  normal foreground process; Ctrl+C ends the session and rotates the
  tokens so any URL that was shared stops working.
* The accepted command surface is enumerated in code on both ends.
  The tunnel relay validates `kind` against an allowlist before
  queuing; the bridge validates again before dispatching. No
  arbitrary shell, no arbitrary file read, no process spawn.
* Every command lands in
  `%LOCALAPPDATA%\ParsecCouchLink\data\logs\lab-mode.log` on the host
  as an audit trail.
* Cleartext Wi-Fi credentials never traverse the tunnel. They live in
  a DPAPI-encrypted vault on the host, decrypted only when the
  operator triggers `wifi_apply_saved` and zeroized immediately after
  the CDC `SET_WIFI` call.

---

## Host: starting a session

```powershell
# One-time: stash your 2.4 GHz Wi-Fi credentials locally so you don't
# have to type them every iteration. DPAPI keeps the blob bound to
# your Windows login.
couchlink save-wifi --ssid 'home2g'      # prompts for the password
# (or pass --password if you really want to)
# (--clear deletes the saved vault)

# Per session:
couchlink lab-mode
```

Lab mode prints two lines and goes silent:

```
Lab session: https://couchlink.whyknot.dev/v/<token>
Ctrl+C to end. Activity log: %LOCALAPPDATA%\ParsecCouchLink\data\logs\
```

Send the URL to the operator out-of-band (DM, email, signal -- it
doubles as the auth, so treat it like a password). Press Ctrl+C any
time to end. The tokens rotate whenever the bridge restarts.

---

## Operator: driving a session

From a separate machine, in PowerShell:

```powershell
iwr https://couchlink.whyknot.dev/client.ps1 -UseBasicParsing | iex
Set-CouchlinkSession 'https://couchlink.whyknot.dev/v/<token>'
Get-CouchlinkInfo
```

`Get-CouchlinkInfo` lists `allowed_kinds` so you can see the live
command set at any time.

### Iteration loop

```powershell
Send-CouchlinkUf2 -Path .\couchlink-pico2w.uf2   # chunked + sha256
Invoke-CouchlinkBootsel                          # CDC -> UDP -> picotool
Wait-Couchlink -Ms 1500                          # BOOTSEL drive mounts
Invoke-CouchlinkFlash                            # flash the upload
Wait-Couchlink -Ms 4000                          # Pico reboots
Get-CouchlinkPicoIdentity                        # CDC HELLO
Invoke-CouchlinkWifiApplySaved                   # vault on the host
Find-CouchlinkPico                               # UDP discovery
Read-CouchlinkPicoLog | Out-Host                 # firmware diag ring
```

For a full forensic snapshot:

```powershell
Invoke-CouchlinkBundle -OutPath .\bundle.zip
```

For situational awareness:

```powershell
Watch-CouchlinkStream                            # live events
Get-CouchlinkLabState                            # bridge state snapshot
Get-CouchlinkStateJournal -Tail 50               # host's state-journal.log
Get-CouchlinkBridgeLog -Tail 200                 # today's bridge log
Test-CouchlinkPing                               # relay + bridge RTT
```

---

## Accepted commands

The relay rejects any other `kind` with a 400 before queueing. The
bridge enforces the same list as a second gate.

| `kind`                | Purpose                                                            |
|-----------------------|--------------------------------------------------------------------|
| `upload_uf2`          | Upload a UF2 to the host's lab-mode slot, chunked.                 |
| `flash`               | Flash the most recently uploaded UF2 onto a mounted BOOTSEL drive. |
| `force_bootsel`       | Try CDC, then UDP, then `picotool reboot -u -f` to enter BOOTSEL.  |
| `doctor`              | Run the bridge's 7-check diagnostic ladder.                        |
| `bundle`              | Build a support zip and stream it back via file_chunk / file_eof.  |
| `identify`            | CDC HELLO. Returns fw triple + board + creds_present + wifi_joined.|
| `discover`            | UDP broadcast on 4242. Returns the first ACK.                      |
| `ping`                | Round-trip nonce echo for relay+bridge liveness.                   |
| `state`               | Bridge state snapshot (version, last Pico, vault presence).        |
| `pull_log`            | UDP TYPE_GET_LOG against the latched Pico's diag ring.             |
| `read_state_journal`  | Tail of `state-journal.log` (1..2000 lines).                       |
| `read_bridge_log`     | Tail of today's tracing log (1..2000 lines).                       |
| `wifi_apply_saved`    | Decrypt the host's DPAPI vault and CDC SET_WIFI in one shot.       |
| `wifi_clear`          | CDC CLEAR_WIFI -- wipe Pico flash creds, re-enter setup mode.      |
| `sleep`               | Bridge-side bounded sleep so scripts can sequence without timers.  |

---

## Limits

* 240 commands per 60 s per session at the relay (sliding window).
* 2 MiB hard cap per command body at the relay.
* 10 session mints per 60 s per source IP at the relay.
* 24 h idle TTL on a session whose bridge has disconnected.
* Wi-Fi vault: SSID up to 32 bytes, password up to 63 bytes (matches
  the firmware's flash_creds_t).
* `read_state_journal` / `read_bridge_log`: 2000-line hard cap.
* `sleep`: 60 000 ms hard cap.

---

## Troubleshooting

| Symptom                                       | Likely cause                                                | Action                                                                       |
|-----------------------------------------------|-------------------------------------------------------------|------------------------------------------------------------------------------|
| 404 on `Get-CouchlinkInfo`                    | View token expired (host restarted the bridge).             | Ask the host for a fresh URL.                                                |
| `bridge_connected: False` persists            | Bridge not running, or host network can't reach the relay.  | Have the host restart `couchlink lab-mode`. Check their firewall / DNS.      |
| 400 with `kind not in lab-mode allowlist`     | Unknown `kind` in the POST body.                            | Check the live list at `Get-CouchlinkInfo.allowed_kinds`.                    |
| 413 on `Send-CouchlinkUf2`                    | Single chunk over 2 MiB.                                    | Re-run with `-ChunkBytes 131072` (or smaller).                               |
| 429 from the relay                            | More than 240 commands in 60 s on this session.             | Back off; sequence with `Wait-Couchlink -Ms`.                                |
| `bootsel_result.ok == false` for all methods  | Pico USB is fully wedged.                                   | Ask the host to physically power-cycle the Pico.                             |
| `identify` reports `no setup-mode Pico found` | Pico is in run mode (XInput, not CDC).                      | Run `force_bootsel` then `flash`, or `wifi_clear` then power-cycle.          |
| `wifi_apply_saved` says `no saved Wi-Fi`      | Host hasn't populated the vault yet.                        | Ask the host to run `couchlink save-wifi` on their own terminal.             |
