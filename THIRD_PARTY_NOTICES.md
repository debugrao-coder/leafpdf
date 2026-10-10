# Third-party notices

LeafPDF 0.1.0 source code and its generated project icon are licensed under MIT; see `LICENSE`. Third-party code remains subject to its own terms. This document identifies the bundled components and does not replace their complete license texts.

## PDFium runtime

The portable distribution includes an unmodified Windows x64 `pdfium.dll` from [bblanchon/pdfium-binaries, release chromium/8086](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F8086), version 157.0.8086.0, built without V8 or XFA. The exact download and checksums are recorded in `scripts/pdfium.json` and in the package's `build-info.json`.

The binary-build project is licensed under MIT (Copyright 2014-2025 Benoit Blanchon), preserved as `licenses/pdfium-binaries-LICENSE.txt`. PDFium itself carries a BSD 3-Clause license (Copyright 2014 The PDFium Authors); the complete upstream `licenses/pdfium.txt`, including its additional appended terms, is preserved as `licenses/pdfium/pdfium.txt`.

All 17 notice files supplied in the pinned archive are copied without alteration to `licenses/pdfium/`:

| Component | Notice file |
| --- | --- |
| PDFium | `pdfium.txt` |
| Abseil | `abseil.txt` |
| AGG | `agg23.txt` |
| Dragonbox | `dragonbox-Boost.txt`, `dragonbox-Apache2-LLVM.txt` |
| fast_float | `fast_float.txt` |
| FreeType | `freetype.txt` |
| HarfBuzz | `harfbuzz.txt` |
| ICU and its included data/components | `icu.txt` |
| Little CMS | `lcms.txt` |
| libjpeg-turbo / Independent JPEG Group | `libjpeg_turbo.md`, `libjpeg_turbo.ijg` |
| libpng | `libpng.txt` |
| OpenJPEG | `libopenjpeg.txt` |
| LLVM libc | `llvm-libc.txt` |
| simdutf | `simdutf.txt` |
| zlib | `zlib.txt` |

This software uses the FreeType Project, including the work of David Turner, Robert Wilhelm, Werner Lemberg, and the FreeType Team. The complete FreeType copyright notices and disclaimer are in `licenses/pdfium/freetype.txt`.

This software is based in part on the work of the Independent JPEG Group.

The supplied ICU notice includes notices for source build tools, with their stated distribution exceptions. All supplied notices are retained; their presence does not indicate that every separately named tool is embedded in the DLL.

Upstream references: [PDFium license](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/8086/LICENSE), [PDFium binary-build license](https://github.com/bblanchon/pdfium-binaries/blob/master/LICENSE), [FreeType license](https://freetype.org/license.html), [libjpeg-turbo license](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/main/LICENSE.md).

## Rust dependencies

`Cargo.lock` fixes the Rust dependencies for this version. Packaging locates crates using `cargo metadata`, copies every top-level `LICENSE*`, `COPYING*`, and `NOTICE*` file and any explicit Cargo `license-file`, and fails if license information is missing. Complete texts are in `licenses/rust/<crate>-<version>/`; `licenses/rust-dependencies.json` records every crate, its SPDX expression, upstream repository, and hashes of copied notices.

Most crates offer MIT or Apache-2.0. The following additional expressions are preserved rather than reduced to MIT:

| Crate | License expression |
| --- | --- |
| adler2 | `0BSD OR MIT OR Apache-2.0` |
| bytemuck | `Zlib OR Apache-2.0 OR MIT` |
| byteorder-lite, memchr | `Unlicense OR MIT` |
| miniz_oxide | `MIT OR Zlib OR Apache-2.0` |
| moxcms, pxfm | `BSD-3-Clause OR Apache-2.0` |
| unicode-ident | `(MIT OR Apache-2.0) AND Unicode-3.0` |
| zlib-rs | `Zlib` |

The application uses Microsoft's system Win32 APIs. Windows itself is not included in the distribution. Python, Pillow and ReportLab are build/test tools; their Python packages are not included in the portable application.

## Rust standard library

The executable also incorporates the Rust standard library. Its complete `COPYRIGHT-library.html` and the accompanying `share/doc/rust/licenses/` texts from the actual compiler's sysroot are preserved in `licenses/rust-std/`. This includes MIT, Apache-2.0 and the additional license texts referenced by Rust's notices. These files are supplied by the `rustc` component; the full `rust-docs` component is not required.

The compiler version, commit, upstream source URL and copied-file hashes are recorded in `build-info.json`. The complete copyright document identifies which terms apply to which Rust components; the presence of a license text alone does not apply that license to all of LeafPDF.
