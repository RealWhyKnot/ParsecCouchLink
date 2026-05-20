# Remote Sessions

The bridge has a built-in debug tunnel that lets someone you trust look
at your running bridge over the network, without you having to send log
files back and forth or ship a fresh build each time the diagnosis
shifts. It works equally well for a friend giving a hand, a support
contact, or you reaching your own machine from a second PC.

The other side runs a couple of PowerShell commands on their own
machine. The bridge on your machine relays a bounded set of debug
commands. You watch the whole thing happen in your console.

## What the tunnel can do

The remote operator can ask your bridge to run any of these executables
on your PC:

| Executable          | What it lets them do                                                                  |
|---------------------|---------------------------------------------------------------------------------------|
| `couchlink`         | Any CouchLink subcommand: doctor, bundle, logs, configure-wifi, flash, test.          |
| `cmake` / `ninja`   | Rebuild the Pico firmware from your local copy, if you have one.                      |
| `dir` / `ls`        | List a directory you can already read.                                                |
| `type` / `cat`      | Print a small file.                                                                   |

They can also read a small whitelist of bridge files (the bridge log for
today, the state journal, the Pico diag stub, the bridge config), and
trigger a few runtime actions (drop a stale UDP peer, pull the
firmware's log ring over UDP, dial up the bridge's own log verbosity).

## What the tunnel cannot do

- It is **not** a general shell. They cannot run arbitrary commands
  like `pwsh`, `bash`, `cmd`, `git`, network tools, package managers, or
  editors. The bridge refuses anything not in the allowlist.
- It cannot read files outside the whitelist above. There is no
  "give me `C:\Users\Me\Documents\...`" path.
- It cannot install software.
- It cannot rebuild or restart the bridge itself (that would kill the
  connection). Firmware rebuild + flash works fine; bridge updates are
  still a hands-on action on your machine.

The bridge redacts known-sensitive fields (Wi-Fi password, the tunnel
write token) from any file it sends out.

## Starting a session

On your PC:

```powershell
.\couchlink.exe tunnel start
```

Output looks like:

```
tunnel session ready.
  server   : https://couchlink.whyknot.dev
  view url : https://couchlink.whyknot.dev/v/<32 chars>
```

Send the **view URL** to the remote operator. Treat it like a password
-- anyone who gets that URL can run commands against your bridge.

If the bridge is not already running, start it (`couchlink.exe`, no
arguments) so the other side actually has something to talk to.

## Watching what they do

Your normal bridge log (the one `couchlink.exe logs` prints) gains an
extra entry for every command the remote operator runs and every line
of output:

```
tunnel cmd [c_abc123] exec couchlink doctor
[c_abc123] -- bridge v2026.5.20.0-beta is running
[c_abc123] -- Pico fw v2026.5.20.0 @ 192.168.1.7 (last seen 0.4 s ago)
...
tunnel exec [c_abc123] exit 0 (1843 ms)
```

There is also a browser view of the same stream at the view URL you
shared. Open it in your own browser if you want a side-by-side view.

## Ending a session

There is no explicit "logout". Two ways to revoke:

```powershell
.\couchlink.exe tunnel disable      # clears the saved session, restart bridge to apply
```

Or just restart the bridge -- the session tokens rotate and the old URL
goes dead immediately. Sessions also auto-expire after 24 hours of
bridge idle even if nobody revokes them.

## What to tell the remote operator

They need a PowerShell prompt (Windows PowerShell 5.1, built into
Windows 10 and 11, works fine; PowerShell 7 also works) and the view
URL you sent. Their setup is one line:

```powershell
iwr https://couchlink.whyknot.dev/client.ps1 -UseBasicParsing | iex
Set-CouchlinkSession '<view-url>'
Get-CouchlinkInfo
```

From there they have a few client commands:

```powershell
clink couchlink doctor              # run a CouchLink subcommand and stream the output
Read-CouchlinkFile -Key state_journal
Watch-CouchlinkStream               # tail every event as it happens
```

Full reference for them, including the troubleshooting table:
<https://couchlink.whyknot.dev/USAGE.md>

## Safety reminders

- **The URL is the password.** Do not share it publicly. Do not paste
  it into a chat with strangers. Do not commit it to a repo.
- The bridge runs as you on your machine, so the remote operator can
  read what you can read, including your bridge logs. If your logs
  contain anything you do not want them to see, copy out the relevant
  lines and share those by hand instead.
- Closing the bridge is a hard revoke. If at any point you are
  uncomfortable, hit Ctrl-C on the bridge and their URL stops working
  before the next command they can send.
