# Third-party notices

This application embeds PDFium. The distributed Windows x64 binary is from
`bblanchon/pdfium-binaries`, release `chromium/8152`, file `pdfium-win-x64.tgz`.
Its build/package license is reproduced in `PDFium-LICENSE.txt` (source: `assets/LICENSE`).
PDFium and its bundled components carry additional notices reproduced in `licenses/`
(source: `assets/licenses/`). Keep these notices with redistributions.

PDFium DLL SHA-256:
`BA884CC5C6F2C3CD7C2ECCA9DCE36C2E341F78EB586ABD691A2BBC017AA9871E`.

Rust dependencies include egui/eframe, winit, glutin, tray-icon/muda, rfd,
windows-sys, regex, serde/serde_json, sha2, strsim, tempfile, and libloading,
plus their transitive dependencies. Exact versions are fixed in Cargo.lock.
The distribution includes Rust dependency license texts under `licenses/rust/`
and an inventory in `licenses/RUST-DEPENDENCIES.txt`.

The application embeds the unmodified JetBrains Mono Regular font, version
2.304, from the official JetBrains Mono project. Copyright 2020 The JetBrains
Mono Project Authors. It is licensed under the SIL Open Font License 1.1,
reproduced in `licenses/JetBrainsMono-OFL.txt`
(source: `assets/licenses/JetBrainsMono-OFL.txt`). The font file is included
in the source distribution under `assets/fonts/JetBrainsMono-Regular.ttf`.

No Java runtime, Spring Boot, or Apache PDFBox binary is included in this application.
