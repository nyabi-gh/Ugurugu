# Contributing to Ugurugu

## Licensing of contributions

Ugurugu is released under the GNU General Public License, version 3 or (at
your option) any later version — SPDX identifier `GPL-3.0-or-later`.

Contributions are accepted on the same terms. By opening a pull request you
confirm that:

- you wrote the contribution yourself, or you have the right to submit it
  under this license;
- you license it under `GPL-3.0-or-later`, so the project can keep
  distributing it under that license and under later versions of the GNU GPL
  as they are published;
- you keep the copyright to your own work. Ugurugu asks for no copyright
  assignment.

Inbound contributions therefore carry the same license as outbound releases.
This is what keeps the project able to move to a future version of the GNU
GPL without collecting permission from every past contributor.

## License headers

Every tracked `.rs` and `.js` file starts with:

```
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) <year> <your name>
```

Python and PowerShell tools start with the same two lines as `#` comments.
New files need them too. CI fails the `Format and release metadata` job when a
tracked source file is missing the SPDX identifier.

## Dependencies

A dependency whose license is incompatible with `GPL-3.0-or-later` cannot be
linked in. For Ugurugu 3, `cargo deny` checks crate licenses and advisories
against `deny.toml`, `tools/native_deps.py` rejects crates that build or link
native code not on its approved list, and the package's
`THIRD_PARTY_NOTICES.txt` is generated from `about.toml` by
`tools/third_party_notices.py`. Code or assets that no crate license covers
are added in that script.

## Before you open a pull request

Ugurugu 3 (`apps/`, `crates/`) is the maintained app. The C++ 2.x sources
are at the `v2.2.13` tag.

- Run the checks in [BUILDING.md](BUILDING.md#checks).
- Write interface text in `apps/ugurugu/i18n/strings.txt` with Korean and
  Japanese translations, then generate the `.ftl` files with
  `python -X utf8 tools/strings_to_ftl.py`. Do not edit the `.ftl` files by hand.
- Add a release note to `release-notes/<version>.en.md` when the change is
  user visible. Release notes are written in English only. End each entry
  with the issues it answers and the pull requests that made it, in
  parentheses, issues first: `(#64, #66)`. GitHub links them in the release
  and adds its list of the pull requests merged since the previous one.
