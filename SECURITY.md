# Security policy

Ugurugu opens files that other people made — `.ugu` projects, `.wawa` drawings
from WiggleWiggleTool, `.wwpreset` presets, and imported images. A bug in that
code can be more than a crash, so security reports get their own channel.

## Supported versions

Only the latest release is supported. Ugurugu updates itself, and fixes ship in
the next release rather than as patches to older versions.

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Anything older | No — please update first |

Check **Help → Check for Updates** to confirm you are on the latest version
before reporting.

## Reporting a vulnerability

**Do not open a public issue for a security problem.** The normal issue guide
asks for the `.ugu` file that reproduces the bug, and for a security bug that
attachment is a working exploit.

Report it privately instead, either way:

- [Open a private security advisory](https://github.com/nyabi-gh/Ugurugu/security/advisories/new)
  on GitHub — preferred, because the discussion and the fix stay in one place.
- Email <contact@nyabi.dev> with `Ugurugu security` in the subject if you would
  rather not use GitHub.

Helpful things to include:

- the Ugurugu version and your operating system;
- what an attacker gains — crash, memory corruption, file read or write outside
  the document, code execution;
- the smallest file or step sequence that reproduces it. Attach it to the
  private advisory, not to a public issue.

## What to expect

Ugurugu is maintained by one person, so please read these as honest intentions
rather than a contractual SLA:

- an acknowledgement within 7 days;
- an assessment, and whether a fix is planned, within 30 days;
- a fix in the next release once one is ready.

You will be credited in the advisory and the release notes unless you ask not
to be. There is no bug bounty — the project has no funding for one.

Please give the fix a chance to ship before disclosing publicly. If a report
goes unanswered past the windows above, disclosing is a reasonable response and
not something you need permission for.

## In scope

- Opening or importing a file — `.ugu`, `.wagle`, `.wobble`, `.wawa`,
  `.wwpreset`, or an imported image — that causes memory corruption, code
  execution, or reads and writes outside the document.
- Anything that lets a crafted file or a network position influence what the
  built-in updater installs.
- Recovery, autosave, or export writing outside the folder the user chose,
  including through a path taken from a file.
- Exposure of file contents or paths to another program or another user on the
  same machine.

## Out of scope

- Attacks that already require the ability to run code as your user account, or
  to write to Ugurugu's installation directory. At that point the machine is
  compromised regardless of Ugurugu.
- A plain crash or hang with no memory-safety consequence. Those are welcome as
  ordinary [issues](https://github.com/nyabi-gh/Ugurugu/issues).
- Unfixed vulnerabilities in Qt, libwebp, spdlog, Sparkle, or Velopack. Report
  those upstream — but do tell us if Ugurugu ships an affected version, and we
  will bump it. Bundled versions are recorded in
  [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- The unrecognized-app warning Windows shows for the installer, and anything
  else that follows from the project not buying a code-signing certificate.
  This is a known cost tradeoff, not an oversight.
- Reports from an automated scanner with no explanation of the impact on
  Ugurugu.
