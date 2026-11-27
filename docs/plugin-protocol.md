# Local plugin protocol

The shared protocol is `parallaxe.plugin/v1`. A plugin is a directory containing one UTF-8 `manifest.json`. The complete manifest is the executable content. No JavaScript, dynamic libraries, external programs, filesystem API, network API, credentials, or report objects are exposed. Resolver results and hook messages are display suggestions. They must not influence core verdicts or exit codes.

```json
{
  "schema": "parallaxe.plugin/v1",
  "id": "example-badge",
  "name": "Example badge",
  "version": "1.0.0",
  "author": "Example author",
  "license": "MIT",
  "source": "https://example.com/example-badge",
  "permissions": ["report_counts"],
  "operations": [
    {"type": "coverage_badge", "prefix": "Coverage "},
    {"type": "resolver", "input": "short", "output": "Short label"},
    {"type": "hook", "event": "complete", "text": "Finished"}
  ],
  "pages": [{"id": "overview", "title": "Overview", "components": [
    {"type": "heading", "text": "Overview"},
    {"type": "callout", "text": "Local display only"}
  ]}]
}
```

This is a schema illustration, not a distributed plugin or a third-party attribution. Real plugins must provide their own truthful author, source and license. Unknown fields, capabilities, and permissions fail validation. The sole permission is `report_counts`. Without it, coverage operations are rejected. The host supplies only aggregate covered and selected counts. Operations have fixed bounds and no loops, callbacks, external calls or mutable plugin state. A `Runtime` opens its circuit after three consecutive errors and resets its error count after success. Each invocation validates the approved manifest digest and constructs fresh output.

Pages are data shared by desktop and Web. Available component types are `heading`, `badge`, `callout`, `markdown`, `code`, `progress`, `table`, `form`, and `button`. Forms and buttons are static descriptions in this protocol version. The host is responsible for safe rendering and event handling. A page, badge, or returned text cannot execute code. Markdown rendering must reject embedded HTML and unsafe links. There is no browser DOM or egui access.

CLI examples, with explicitly chosen settings file and local directory:

```text
parallaxe plugin verify PATH
parallaxe plugin enable PATH --settings SETTINGS --approve-sha256 SHA256_FROM_VERIFY
parallaxe plugin list --settings SETTINGS
parallaxe plugin disable ID --settings SETTINGS
```

The SHA-256 is calculated over the exact bytes of `manifest.json`. `verify` and `list` do not create files. Enabling stores the digest and canonical local directory, and `read_enabled` checks the ID and digest again. Hosts must check the approved digest on every invocation and reject changed content. The directory path is local user data and must not be included in public diagnostics.

`settings::Settings` is a serializable shared value. Its native `load` and `update` APIs accept an explicit file path. The update API locks around read-modify-write and appends a bounded JSON record. A missing file reads as defaults. Partial trailing writes preserve the previous complete record for reads, but updates fail until the file is repaired. Web applications must supply their own storage adapter. The settings API validates hard scheduling limits, but a GUI must map its QoS profile and schedule preferences explicitly.

The plugin module is excluded on Android and iOS. UI exposure, local directory management, report integration, page hosting, examples, and the public third-party plugin notices are separate integration work. Merely enabling a manifest does not execute it or connect it to a report.

## Samples, packaging and public notices

- `plugins/samples/` holds two first-party examples: `light-report-badge` (coverage badge, resolver mapping, three hook texts) and `heavy-report-guide` (one read-only page covering all nine component types). Their README lists the known limits, including that plugin text is not localized.
- `THIRD_PARTY_PLUGINS.md` is the public provenance table. Every shipped manifest needs a row with author, license, source, permissions and the SHA-256 of its exact bytes; `tools/verify_plugins.py` fails when a row is missing, extra, or stale, and `crates/core/tests/plugin_samples.rs` repeats the digest and shape checks through the real loader.
- `tools/pack_plugin.py` produces an optional `<id>.plugin` bundle: a stored ZIP holding only `manifest.json`, with a fixed timestamp so packing is byte-for-byte reproducible, plus a `sha256sum -c` sidecar over the manifest bytes. The host still reads directories only, so a bundle must be unpacked before it can be enabled.
- Settings that point at plugin directories are local user data. `plugin_directories` stores canonical local paths and must never appear in reports, diagnostics or public documentation.
