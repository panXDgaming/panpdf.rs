<div align="center">

<img src="https://raw.githubusercontent.com/__REPO__/__TAG__/.github/logo.png" width="90" alt="">

# PanPDF __VERSION__

**A PDF editor that edits the file.** Your document keeps its own state: the
same objects, the same fonts, the same protection, the same bytes everywhere
you did not touch.

</div>

> ### This is a development release
> It runs, it opens and edits real documents, and it is not finished. Keep a
> copy of anything you care about, and tell us what broke:
> [open an issue](https://github.com/__REPO__/issues/new).

## What's new

- **Install with one command.** Windows (PowerShell):
  `irm https://panpdf.org/install.ps1 | iex` -- Linux and macOS:
  `curl -fsSL https://panpdf.org/install.sh | sh`. No administrator rights,
  a Start menu or applications-menu entry, and run it again to update.
- **Every converter inside the program.** A Tools room on the home screen
  and in the menus: PDF to Word, Excel, PowerPoint, JPG, HTML, Markdown and
  text, and back; compress, repair, OCR, PDF/A, protect, unlock, sign,
  redact and compare -- all on your computer, nothing uploaded.
- **An assistant that finishes the job.** It plans a whole task, carries it
  out, and everything it did can be undone in one step. It can convert,
  make scanned pages searchable, add links, shapes and form fields, write a
  new document from nothing -- and draw: a cartoon for a lesson, a diagram,
  a sign. A "Get a key" link beside each provider (Gemini's is free).
- **Nineteen shapes.** Rectangle, rounded rectangle, oval, triangles,
  diamond, parallelogram, trapezoid, pentagon, hexagon, octagon, star,
  heart, cross, block arrow, speech bubble, cloud, moon and lightning.
- **Markdown, all of it.** Documents the assistant writes read every example
  of CommonMark 0.31.2, with GitHub's tables, task lists and
  strikethrough, footnotes, highlight, superscript and subscript, notes and
  simple HTML -- no stray marks on the page.
- **Read first, edit when you choose.** A document opens for reading;
  the Edit button brings every tool. A protected document asks about its
  restrictions only when you start editing.
- **Calmer windows.** Menus without repeats, quieter dialogs that can be
  moved, and High quality or Fast and light text recognition.

## Download

| Architecture | Windows | Linux | macOS |
| --- | --- | --- | --- |
| **x86-64 (64-bit)** | [Setup](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-windows-x86_64-setup.exe) · [ZIP](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-windows-x86_64.zip) | [DEB](https://github.com/__REPO__/releases/download/__TAG__/panpdf___VERSION___amd64.deb) · [RPM](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-1.x86_64.rpm) · [AppImage](https://github.com/__REPO__/releases/download/__TAG__/PanPDF-__VERSION__-x86_64.AppImage) · [tar.gz](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-linux-x86_64.tar.gz) | [DMG](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-macos.dmg) |
| **ARM64 (Apple Silicon)** | — | — | [DMG](https://github.com/__REPO__/releases/download/__TAG__/panpdf-__VERSION__-macos.dmg) |

The macOS disc image holds one program that runs on both architectures, so both
rows point at the same file.

## Which one do I want?

- **Windows** -- take the Setup. It asks where to put the program, shows what
  it is doing, adds it to the Start menu and to Settings, and uninstalls
  cleanly; installing a newer version over an older one takes the old one's
  files with it. Take the ZIP instead if you would rather unpack it somewhere
  yourself and run `panpdf.exe`, or cannot install anything.

  Neither is signed yet. Windows will warn you when you download the Setup --
  the button offered is *Delete*, and *Keep* is in the `...` menu -- and it
  will warn again the first time you open the ZIP's `panpdf.exe`. That is what
  an unsigned program looks like, and it is on the list to fix. The installer
  is not a way around it.
- **Debian, Ubuntu, Mint** -- the DEB. `sudo dpkg -i panpdf_*.deb`
- **Fedora, RHEL, openSUSE** -- the RPM. `sudo rpm -i panpdf-*.rpm`
- **Any other Linux** -- the AppImage: `chmod +x` it and run it. Nothing is
  installed.
- **A Linux machine you cannot install on** -- the tar.gz. Unpack and run
  `bin/panpdf`; the fonts it needs travel beside it.
- **macOS** -- the DMG. It is not notarised yet, so the first launch needs
  right-click → Open.

It needs nothing installed beside it. The Windows build uses only the libraries
Windows ships, and the Linux packages ask for what a desktop already has.
Optional: `tesseract-ocr` to read scanned pages, `zenity` for the system file
dialogue.

## Checking what you downloaded

`SHA256SUMS` below is part of this release. Every file above is listed in it.

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

## Source

Building it yourself is three commands, and the whole of it is in this
repository -- see the [README](https://github.com/__REPO__#build-it-yourself).
Licence: AGPL-3.0.

---
