# Third-party components

Mangix itself is MIT (see `LICENSE`). It is built on, and distributed with,
the following.

## Slint — GUI toolkit

Copyright © SixtyFPS GmbH. Used under the **Slint Royalty-free License**,
which permits use in proprietary desktop applications at no cost provided the
use of Slint is disclosed. Mangix discloses it in the settings panel
("Made with Slint"). Slint is also available under GPLv3 and commercial
licences: https://slint.dev/pricing

Choosing the Royalty-free option is what allows Mangix's own source to stay
MIT rather than being GPL as a whole. If you remove the attribution, that no
longer holds — switch to GPLv3 for the whole project, or buy a commercial
licence.

## PDFium — PDF rendering

Copyright 2014 The PDFium Authors. BSD-3-Clause. Distributed as a separate
`pdfium.dll` alongside the executable, not linked into it. The licence text
below must accompany any binary distribution that includes the library.

```
Copyright 2014 The PDFium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

    * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
    * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

Prebuilt binaries come from https://github.com/bblanchon/pdfium-binaries.

## Rust crates

All MIT, Apache-2.0, or dual MIT/Apache-2.0, none of which impose obligations
beyond preserving their notices:

| Crate | Purpose |
|---|---|
| `image` | decoding JPEG, PNG, GIF, WebP and BMP |
| `zip` | reading CBZ archives |
| `pdfium-render` | bindings to PDFium |
| `rfd` | native file dialogs |
| `anyhow` | error handling |
| `winresource` | embedding the icon on Windows |

Run `cargo tree --format "{p} {l}"` for the full resolved set with licences.

## Not bundled

7-Zip and unrar are invoked as external programs when present. Mangix neither
links nor ships them, so their licences — including the unRAR restriction —
do not attach to this project.
