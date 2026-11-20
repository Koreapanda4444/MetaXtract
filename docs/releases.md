# Desktop release notes

[English](releases.md) | [한국어](releases_KR.md)

## 0.2.0

The desktop adds keyboard shortcuts and searchable commands, grouped export/security/case/diagnostic tools, guided CSV/JSONL/HTML/JSON delivery, form-based redaction with preview, report/signature/verification review, reusable presets and persistent task/error history. Update checking is manual and does not send evidence or install software automatically. Cached summaries/timeline filtering and context-bound background results improve responsiveness and case transitions.

| Platform | Distribution |
|---|---|
| Windows x64 | MetaXtract.exe launcher, portable ZIP and per-user NSIS Setup, embedded app/installer icon and version resources, Start-menu and optional desktop shortcuts, owned-file uninstall |
| macOS ARM64 | Application bundle metadata/icon, DMG and optional Developer ID/notarytool support |
| Linux x64 | Desktop/menu integration, AppImage and DEB; Ubuntu 24.04 build baseline and measured runtime ABI requirements |

The internal CLI/GUI/worker layout preserves fixed worker lookup and bundled libraries. Source archives and original native/Rust/AppImage notices accompany checksum-verified downloads, including libfuse/runtime sources and relinking instructions.

Release acceptance includes all three native builds, extraction contracts and sanitizers, packaged GUI scan/search/inspection/analysis/export workflows, case/schema compatibility, and native installation/reinstallation/removal checks. See the linked CI receipts for the exact build commit and results.

Windows downloads are not Authenticode signed. Standard macOS CI downloads use ad-hoc signatures and are not notarized. Developer ID/notarization requires separately configured credentials. No new metadata extraction formats are introduced by this desktop/distribution release.
