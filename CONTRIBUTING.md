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

Every tracked `.cpp`, `.hpp`, `.mm`, `.vert`, `.frag`, and `.js` file starts
with:

```
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) <year> <your name>
```

New files need the same two lines. CI fails the `Format and release metadata`
job when a tracked source file is missing the SPDX identifier. `src/main.cpp`
carries the full GNU notice in addition, and it is the copy that states the
warranty disclaimer in source form.

When you add a third-party dependency, record it in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and install its license text
from `cmake/UguruguPackaging.cmake` so release packages ship it. A dependency
whose license is incompatible with `GPL-3.0-or-later` cannot be linked in.

## Before you open a pull request

- Build and run the tests as described in [BUILDING.md](BUILDING.md).
- Run `npm run check` in `web/` when you touch the web shell.
- Format sources with `clang-format` using the repository `.clang-format`.
- Add Korean and Japanese translations for every new `tr()` string. CI
  rejects `.ts` files that still contain unfinished entries.
- Add a release note to `release-notes/<version>.en.md` when the change is
  user visible. Release notes are written in English only.
