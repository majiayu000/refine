# RustSec warning baseline

This document records the non-vulnerability warnings emitted by
`cargo audit 0.22.2` on 2026-08-13. They remain visible in CI and are not
suppressed through an ignore list. `cargo audit` still exits non-zero for a
vulnerability, so the CI job prevents a new vulnerable dependency from being
merged while these upstream warnings are tracked separately.

## Current dependency paths

| Advisory | Crate | Target | Dependency path |
| --- | --- | --- | --- |
| `RUSTSEC-2024-0429` | `glib 0.18.5` | Linux desktop only | `refine-desktop -> tauri/tauri-runtime-wry -> gtk/webkit2gtk -> glib` |
| `RUSTSEC-2024-0413` | `atk 0.18.2` | Linux desktop only | `refine-desktop -> tauri -> gtk -> atk` |
| `RUSTSEC-2024-0416` | `atk-sys 0.18.2` | Linux desktop only | `refine-desktop -> tauri -> gtk/webkit2gtk -> atk-sys` |
| `RUSTSEC-2024-0412` | `gdk 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> wry/gtk -> gdk` |
| `RUSTSEC-2024-0418` | `gdk-sys 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> wry/tao/gtk -> gdk-sys` |
| `RUSTSEC-2024-0411` | `gdkwayland-sys 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> tao -> gdkwayland-sys` |
| `RUSTSEC-2024-0417` | `gdkx11 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> wry -> gdkx11` |
| `RUSTSEC-2024-0414` | `gdkx11-sys 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> wry/tao -> gdkx11-sys` |
| `RUSTSEC-2024-0415` | `gtk 0.18.2` | Linux desktop only | `refine-desktop -> tauri/tauri-runtime-wry -> gtk` |
| `RUSTSEC-2024-0420` | `gtk-sys 0.18.2` | Linux desktop only | `refine-desktop -> tauri-runtime-wry -> gtk/webkit2gtk -> gtk-sys` |
| `RUSTSEC-2024-0419` | `gtk3-macros 0.18.2` | Linux desktop only | `refine-desktop -> tauri -> gtk -> gtk3-macros` |
| `RUSTSEC-2024-0370` | `proc-macro-error 1.0.4` | Linux desktop only | `refine-desktop -> tauri -> gtk -> glib-macros/gtk3-macros -> proc-macro-error` |
| `RUSTSEC-2025-0081` | `unic-char-property 0.9.0` | All desktop build targets | `refine-desktop -> tauri/tauri-build -> tauri-utils -> urlpattern -> unic-ucd-ident -> unic-char-property` |
| `RUSTSEC-2025-0075` | `unic-char-range 0.9.0` | All desktop build targets | `refine-desktop -> tauri/tauri-build -> tauri-utils -> urlpattern -> unic-ucd-ident -> unic-char-property -> unic-char-range` |
| `RUSTSEC-2025-0080` | `unic-common 0.9.0` | All desktop build targets | `refine-desktop -> tauri/tauri-build -> tauri-utils -> urlpattern -> unic-ucd-ident -> unic-ucd-version -> unic-common` |
| `RUSTSEC-2025-0100` | `unic-ucd-ident 0.9.0` | All desktop build targets | `refine-desktop -> tauri/tauri-build -> tauri-utils -> urlpattern -> unic-ucd-ident` |
| `RUSTSEC-2025-0098` | `unic-ucd-version 0.9.0` | All desktop build targets | `refine-desktop -> tauri/tauri-build -> tauri-utils -> urlpattern -> unic-ucd-ident -> unic-ucd-version` |

The GTK3 bindings are unmaintained and the `glib` iterator advisory is marked
unsound, but both are controlled by Tauri's current Linux WebView backend.
There is no newer compatible `glib 0.18` release, so replacing this family
requires an upstream Tauri/WebKit backend change rather than a lockfile-only
update here. The five `unic-*` crates are controlled by Tauri's transitive
`urlpattern 0.3.0`; the application does not depend on `urlpattern` directly.

`cargo tree --target aarch64-apple-darwin -i glib@0.18.5` returns no path,
confirming that the GTK/glib warning family is not linked into the macOS
desktop target. The `unic-*` path is cross-platform build tooling.

## Upgrade policy

1. Keep Tauri and its official plugins on the latest compatible release.
2. On each Tauri update, rerun the commands below and remove this baseline row
   as soon as the transitive path disappears.
3. Do not add blanket `cargo audit` ignores for these warnings.
4. Treat any advisory classified as a vulnerability as a blocking CI failure.
5. If the `glib` unsound API becomes reachable from application code before an
   upstream fix exists, disable the affected Linux surface or apply a narrowly
   reviewed upstream patch instead of suppressing the advisory.

## Reproduction

```bash
cargo audit
cargo update -p tauri --dry-run
cargo update -p glib@0.18.5 --dry-run
cargo tree --target x86_64-unknown-linux-gnu -i glib@0.18.5
cargo tree --target aarch64-apple-darwin -i glib@0.18.5
cargo tree --target all -i proc-macro-error@1.0.4
cargo tree --target all -i unic-char-property@0.9.0
```

At the recorded baseline, Tauri `2.11.5` is the latest published release and
both targeted dry runs report no compatible dependency update.
