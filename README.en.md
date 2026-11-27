<p align="center">
  <img src="assets/icons/parallaxe-box.png" alt="Parallaxe box" width="88" />
</p>

# Parallaxe box

**General-purpose network environment inspector (with Google full-service region signals) · Anonymous read-only diagnostics · Safe rule export · Release builds work out of the box**

[中文 README](README.md) · [CHANGELOG](CHANGELOG.md) · [Quick Start](QUICKSTART.md) · [FAQ](FAQ.md)

> 🧪 **Current version: first public beta `v0.1.0-beta.1`**  
> Desktop (Windows GUI/CLI/Agent), Web (WASM), Android (dual-ABI APK), and Linux (musl headless CLI) ship at the same version.
> **The beta does not include system-level blocking**: Guard only offers read-only impact-closure previews and manually exported rules with warnings; it never writes system configuration.
> Untested items are listed in [CHANGELOG](CHANGELOG.md) and the [release checklist](docs/release-checklist.md). Do not rely on beta results in production.

> **Release stance: Conditional Release**  
> This software is provided only for networks you own or are explicitly authorized to test, offering **anonymous, read-only, evidence-first** egress-region observation and link diagnostics.
> **It is not a "streaming unlock script", an "IP purity score", or a proxy client.**
> Read [Usage boundaries and disclaimer](#usage-boundaries) and [compliance](docs/compliance.md) before use.

---

## 1. What problem does it solve?

After switching a cross-border route, you often see a split: Google Search says US, Gemini says region unsupported, Google Books jumps to another country, and Play apps will not download. Legacy scripts probe one or two pages and hand you a pseudo-precise "purity 95" score — they explain none of the disagreement, and may over-block shared domains and break sign-in or Play downloads along the way.

**Parallaxe box brings this back to engineering facts: diagnose egress-region judgments, help you choose a route — it never chooses or switches routes for you.**

1. **Not an unlock script, but a general network environment inspector (Google full-service probing is the core specialty)** — on top of generic link diagnostics (DNS / egress / dual-stack / path / local QoS), it maintains 107 public Google surfaces (12 service families) plus 28 control targets, each evidence-collected independently.
2. **Not an IP-risk aggregator, but per-service, per-signal, per-path evidence** — no pseudo-precise total score, no third-party scoring API by default; every conclusion carries its signal source, attribution layer, confidence, and freshness.
3. **Not a proxy client** — no node switching, no subscription management, no system network changes; zero-packet by default, never reads browser configuration / cookies / account credentials.
4. **It does not pretend shared CDNs can be separated** — it protects sign-in, billing, Play download chains and shared infrastructure (e.g. `gstatic.com`, `googleusercontent.com`), and says `NO_SAFE_BLOCK` when safe blocking is impossible.

---

## 2. Source build vs official release build (important)

The project deliberately distinguishes two distributions with different defaults:

| Distribution | Default mode | Built-in 107 Google targets | Settings → About |
|---|---|---|---|
| **GitHub Releases artifacts** (Windows zip/exe, Android APK, Web, Linux) | **Release mode** (compile-time `distribution-release` feature; gate lifted) | "My authorized edition · 107 items" visible on launch; one confirmation enables real detection | **No** source zero-packet switch shown |
| **git clone + self build** | **Zero-packet mode** (safe baseline: `allow_online=false`, `admission.grants=[]`) | No packets by default; can switch to release mode in Settings → About after tester confirmation | **Shows** "Switch to release build" entry |

- **Release builds work out of the box**: the default rule pack v2 (107 items) is bundled inside the release artifact — **you do not need to find or import a ZIP yourself**. Authorization still requires an explicit confirmation (no silent packets), but obtaining the targets is zero-effort.
- **Two authorization scopes**: when confirming, choose "This session only" (expires on exit) or "Permanent" (saved in the local user state directory, revocable with one click in Settings). Neither writes back to the built-in catalog nor spreads to other URLs.
- **Two independent switches**: "Release mode" and "AI autonomous debugging" are not the same switch. Release mode + AI autonomous ON means the AI can operate fully (still bound by the QoS red lines: 3.5s interval, batching, TCP-first, stop on 403/429).
- **Control targets are not lifted by release mode**: `control.*` still requires user import + F3 authorization; the `full` profile still excludes `user.*`.
- Run `parallaxe build-info --json` to see the binary's distribution marker (`parallaxe.distribution-release/v1` or `parallaxe.source-build/v1`).

---

## 3. What it can and cannot do

### Capabilities

| Capability | Description |
|---|---|
| **Google full-service matrix** | 107 public Google entry points (identity, search, content, Workspace, media, Play, AI, education, cloud, developers, static assets, APIs) + 28 control targets, each independently probed; the `simple` quick profile has 16 items (including Google One) and no time limit by default. Only three scopes remain: Simple 16 / Google all 107 / All 135. |
| **Honest UA and per-target redirect budget** | The **core application** uses one honest research UA everywhere; no browser-identity/TLS/JA3 spoofing, no cookies, no login. A few public pages (e.g. books' csi) return full content only to browser UAs — such signals that require UA spoofing are **out of scope for the core** and are planned for **plugins/external extensions** (off by default, explicitly enabled by the user, who decides compliance); the current plugin protocol is declarative with no script execution and no transport hook yet, so the core reports `UNKNOWN` honestly when it cannot read them. Each target may declare `max_redirects` in its registry row (default 0 = no follow; targets needing region signals from login display pages set it to 1–2); only HTTPS is followed and the hop chain is recorded. Google subdomains judge regions independently (books=CN, youtube/news=US, Play footer region) and are shown side by side without cross-service majority voting. |
| **Seven-layer region attribution** | Separates egress network, service regional policy, account association, billing/Play profile, user preference, device location, cloud resource region; never presents page language or CDN PoP as egress country. |
| **9 honest states** | `OK` / `RESTRICTED` / `BLOCKED` / `REACHABLE_ONLY` / `LOCAL_PATH_ERROR` / `CONFLICT` / `UNKNOWN` / `SIGNAL_STALE` / `UNSUPPORTED`; says `UNKNOWN` when evidence is thin and `CONFLICT` when same-layer signals disagree. |
| **Three frontends, one core** | Desktop GUI (egui/eframe), headless CLI (`parallaxe`), and NDJSON Agent (`parallaxe agent --stdio`) share `parallaxe-core` — no verdict divergence. |
| **AI-initiated detection** | Agent protocol v2, path A (one-click F3 consent) / path B (opt-in autonomous mode); the bridge is a short-lived file in the temp directory, no listening port, with TTL; reports are always redacted. |
| **Four-column read-only link check** | `parallaxe link explain` separates resolver / connection edge / HTTP egress / address family; zero-packet by default; port scanning strictly prohibited. |
| **Guard impact-closure preview** | Local dependency graph detects shared-CDN conflicts and produces a read-only audit; `applied=false`, `rules_written=0`; never changes local configuration. |
| **Custom URL import (GUI)** | Import your own or authorized URLs (`user.*`) and allow-listed check methods; two liability-consent gates, zero-packet by default, one-shot session grant before a run (consumed after use), audited in reports. |
| **Offline self-test and fixtures** | 28 offline self-tests and 9 sanitized fixtures verify verdict logic and three-frontend parity with no network. |
| **Web / Android / Linux** | The same core compiles to a browser page and dual-ABI APKs; Android requests only `INTERNET` and has no plugin surface; Linux ships musl headless CLI archives. |
| **Redacted by default, zero account contact** | Cookie store disabled, empty `Cookie` header sent, `Set-Cookie` dropped; no browser profile / token reads; IPs hidden by default; export to JSON/JSONL/HTML/CSV/MD/TXT. |
| **Five proxy path modes** | Settings → Advanced Network offers Auto (default: probes environment/system proxy/common ports and chooses per host), None, HTTP, SOCKS, and PAC URL. Auto mode uses bounded probes, never scans subnets, and never changes system proxy/routes/DNS. PAC uses a minimal syntax subset and states uncovered calls honestly. When SOCKS transport is unsupported in the current build, it fails explicitly rather than silently going direct. |

### Hard boundaries (explicitly cannot do)

- **No account-side information**: account-linked country, Play billing country, subscription eligibility, age/organization policy, device location are always `NOT_OBSERVABLE_ANONYMOUSLY` / `NOT_COLLECTED`.
- **No unlock / bypass / laundering**: no CAPTCHA solving, no TLS/JA3 spoofing, no browser automation, no login replay.
- **Not a proxy or network modifier**: no subscription enumeration, no node switching, no speed tests, no port scanning, no changes to system proxy/DNS/hosts/firewall.
- **The beta has no system-level blocking**: only Guard read-only previews and manually exported rules (`applied=false`).
- **Source builds have an empty grant list**: plain git-clone builds send zero packets; the built-in gate is lifted only by a release build or an explicit switch plus confirmation.

---

## 4. 5-minute quick start

See [QUICKSTART.md](QUICKSTART.md) for full commands. Requires Rust `1.78+`.

```bash
# Source build (zero-packet by default)
cargo build --release -p parallaxe-cli

# Headless-only build (servers / CI / containers)
cargo build --release -p parallaxe-cli --no-default-features

# Official release build (release pipeline only)
cargo build --release -p parallaxe-cli --features distribution-release

# Offline self-test (expect 28 passed / 0 failed)
parallaxe selftest --offline

# Show the distribution marker
parallaxe build-info --json

# Catalog / explain / offline fixture
parallaxe targets list
parallaxe explain --target google.gemini
parallaxe check --headless --fixture gemini-positive --json

# Quick profile, 16 items (source build: zero packets, exit code 3)
parallaxe check --headless --profile simple --json

# GUI and Agent
parallaxe gui
parallaxe agent --stdio
```

The binary is at `target/release/parallaxe` (`parallaxe.exe` on Windows). To use the finished product, download from GitHub Releases — install and run, no build required.

---

## 5. Documentation map

**Root**: [CHANGELOG](CHANGELOG.md) · [QUICKSTART](QUICKSTART.md) · [FAQ](FAQ.md) · [ARCHITECTURE](ARCHITECTURE.md) · [Third-party notices](THIRD_PARTY_NOTICES.md) · [Third-party plugins](THIRD_PARTY_PLUGINS.md)

**`docs/` topics** (full index at [docs/README.md](docs/README.md)):
- Core model: [Seven-layer region model](docs/google-region-model.md) · [Full-service coverage](docs/google-coverage.md) · [Signals and fixtures](docs/signals.md) · [9 states and exit codes](docs/verdict.md)
- Interfaces and observation: [GUI/CLI guide](docs/gui-headless.md) · [Agent protocol](docs/agent-interface.md) · [Service observation](docs/service-observation.md) · [Custom import](docs/user-import.md)
- Safety: [Guard and shared CDNs](docs/guard.md) · [Dependency graph and path isolation](docs/isolation.md) · [Account isolation and request budget](docs/account-isolation.md)
- Compliance and release: [Platform terms and compliance](docs/compliance.md) · [Release checklist](docs/release-checklist.md) · [Packaging and multi-platform](docs/packaging.md)

---

<a id="usage-boundaries"></a>

## 6. Usage boundaries and disclaimer

### Appropriate use
Legitimate cross-border business and remote work, study and academic exchange, developers accessing public docs and APIs, cross-region collaboration, and troubleshooting on networks you own or are entrusted to manage.

### Disclaimer
> This software is for read-only diagnostics on networks you own or are authorized to test. Its output describes anonymous observations at a specific time, service, and path; it does not represent your actual location, account country, purchase eligibility, IP reputation, or future availability. Local rule previews do not change remote service judgments. Users must comply with applicable law, organizational policy, and target service terms; do not probe targets you are not authorized to test. The authors have no affiliation, sponsorship, or endorsement relationship with the services mentioned (including but not limited to Google, Cloudflare, Netflix, OpenAI, Anthropic). This disclaimer does not exclude liability that cannot lawfully be excluded.

### Prohibited uses
You may not use this software (or derivative deployments / hosted services) for: copyright infringement, bypassing paywalls or subscriptions, cracking DRM; fraud, impersonation, mass registration, spamming, or harassment; evading sanctions or export controls; unauthorized access or out-of-scope scraping / stress testing / abuse of third-party services; or any conduct violating target service terms.

### Open-source license
Original code is dual-licensed **MIT OR Apache-2.0**. The usage boundaries above are project rules and a disclaimer; they do not unilaterally reduce standard OSI license terms. Commercial distribution or cross-jurisdiction deployment requires your own compliance and legal review.
