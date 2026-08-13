# RustSec warning baseline

This document records the non-vulnerability warnings emitted by
`cargo audit 0.22.2` on 2026-08-13. They remain visible in CI and are not
suppressed through an ignore list. `cargo audit` still exits non-zero for a
vulnerability, so the CI job prevents a new vulnerable dependency from being
merged while these upstream warnings are tracked separately.

## Current dependency paths

| Warning family | Target | Dependency path | Current blocker |
| --- | --- | --- | --- |
| `RUSTSEC-2024-0429` (`glib 0.18.5`, unsound iterator implementation) | Linux desktop only | `refine-desktop -> tauri 2.11.5 -> gtk/webkit2gtk -> glib 0.18.5` | Tauri's current Linux WebView stack still uses the GTK3 `0.18` bindings. There is no newer compatible `glib 0.18` release. |
| Ten `RUSTSEC-2024-0411` through `RUSTSEC-2024-0420` GTK3 maintenance warnings | Linux desktop only | `refine-desktop -> tauri 2.11.5 -> gtk 0.18.2` and its `atk`, `gdk`, and `*-sys` crates | The GTK3 bindings are unmaintained, but remain part of Tauri's current Linux backend. Replacing them requires an upstream Tauri/WebKit backend change rather than a lockfile-only update here. |
| `RUSTSEC-2024-0370` (`proc-macro-error 1.0.4`, unmaintained) | Linux desktop only | `gtk3-macros/glib-macros -> proc-macro-error` | This follows the same GTK3 dependency path and cannot be upgraded independently. |
| Five `RUSTSEC-2025-0075` through `RUSTSEC-2025-0100` `unic-*` maintenance warnings | All desktop build targets | `tauri 2.11.5 -> tauri-utils 2.9.3 -> urlpattern 0.3.0 -> unic-* 0.9.0` | `urlpattern` has newer major releases, but Tauri controls this transitive version. The application does not depend on `urlpattern` directly. |

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
