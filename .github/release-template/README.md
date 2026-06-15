# Release-body templates

`Generate-ReleaseNotes.ps1` reads the markdown files in this directory and emits them into the GitHub release body after the generated changelog and file-integrity sections.

## Tokens

Templates can use these strings:

| Token | Example value |
|---|---|
| `{tag}` | `v2026.6.15.0` |
| `{version}` | `2026.6.15.0` |
| `{owner}` | `RealWhyKnot` |
| `{repo}` | `ParsecCouchLink` |
| `{full-repo}` | `RealWhyKnot/ParsecCouchLink` |
| `{commit-sha}` | full 40-character tag commit |
| `{commit-sha-short}` | first 12 characters of the tag commit |
| `{prior-tag}` | previous release tag, when available |
| `{zip-name}` | `ParsecCouchLink-v2026.6.15.0.zip` |

Keep template text plain ASCII. The generator checks the final body before publish.
