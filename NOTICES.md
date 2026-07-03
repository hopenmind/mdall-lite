# Third-Party Notices

MD -> ALL is written in Rust and bundles or builds upon the third-party
components below. Each is used under its own license. Where a license requires
its text to travel with the distribution, that text ships alongside the
component it covers.

## Bundled rendering engine (high-fidelity PDF path)

The optional high-fidelity PDF tier renders through a bundled headless browser
engine. It is distributed under the BSD 3-Clause license. Its full, verbatim
license text, including the required copyright and attribution notices, ships
inside the engine's own folder next to its binaries, as that license requires.

The engine is never required at runtime: the editor and every other export path
work fully offline through the pure-Rust tiers if the engine is absent.

## KaTeX

HTML export embeds KaTeX for in-browser math rendering. KaTeX is licensed under
the MIT License, Copyright (c) Khan Academy and other contributors.

## Typst and bundled math fonts

The pure-Rust PDF and equation-image tiers use the Typst typesetting engine
(Apache License 2.0) together with its bundled open fonts, including New Computer
Modern. Those fonts are distributed under their respective open font licenses
(GUST Font License / SIL Open Font License).

## Spell-check dictionary

The default en_US spell-check dictionary is fetched at packaging time and ships
with its own license file (en_US.license.txt) next to the application.

## Rust crates

This software depends on the Rust standard library and a number of open-source
crates from crates.io, each distributed under permissive licenses (typically the
MIT or Apache-2.0 license). Their license texts are available in their
respective source repositories.
