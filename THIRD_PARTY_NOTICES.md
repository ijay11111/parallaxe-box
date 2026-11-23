# 第三方许可与权利声明（THIRD_PARTY_NOTICES）

> 生成日期：2026-11-22 · 提取来源：`Cargo.lock` + `cargo metadata --format-version 1 --offline --locked`（100% 离线实测提取，零编造）
> 本项目原创代码采用 **MIT OR Apache-2.0** 双重许可（见 `Cargo.toml`）。

---

## 一、提取方法与依赖总览

- **Workspace 内部成员（4 个，原创代码）**：`parallaxe-core`、`parallaxe-cli`、`parallaxe-agent`、`parallaxe-gui`（`MIT OR Apache-2.0`）。
- **外部 Cargo 依赖包总数**：**488 个**（全部 488 个均已从本地 cargo registry 元数据精确提取 SPDX 许可证表达式，待确认项为 **0**）。
- **强传染许可与禁用依赖核查**：依赖树中 **0 个 GPL / AGPL / SSPL 依赖**；零 `openssl` / `native-tls` / `webview` / `tauri` / `cookie_store` 依赖。

### 许可证分布统计

| SPDX 许可证表达式 | 依赖包数量 |
|---|---:|
| `MIT OR Apache-2.0` | 226 |
| `MIT` | 109 |
| `Apache-2.0 OR MIT` | 37 |
| `MIT/Apache-2.0` | 18 |
| `Unicode-3.0` | 18 |
| `Apache-2.0` | 15 |
| `Zlib OR Apache-2.0 OR MIT` | 8 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 7 |
| `MIT OR Apache-2.0 OR Zlib` | 6 |
| `Unlicense OR MIT` | 5 |
| `Zlib` | 3 |
| `ISC` | 3 |
| `BSD-3-Clause` | 3 |
| `BSL-1.0` | 2 |
| `Apache-2.0 OR ISC OR MIT` | 2 |
| `MIT OR Zlib OR Apache-2.0` | 2 |
| `BSD-3-Clause OR Apache-2.0` | 2 |
| `BSD-3-Clause OR MIT OR Apache-2.0` | 2 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 2 |
| `Unlicense/MIT` | 2 |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 |
| `0BSD OR MIT OR Apache-2.0` | 1 |
| `BSD-2-Clause` | 1 |
| `MIT / Apache-2.0` | 1 |
| `Apache-2.0 AND MIT` | 1 |
| `(Apache-2.0 OR MIT) AND BSD-3-Clause` | 1 |
| `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` | 1 |
| `Apache-2.0 / MIT` | 1 |
| `CC0-1.0` | 1 |
| `MIT OR BSD-3-Clause` | 1 |
| `Apache-2.0 AND ISC` | 1 |
| `Apache-2.0/MIT` | 1 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `CDLA-Permissive-2.0` | 1 |

---

## 二、重点第三方许可专项说明

1. **`r-efi`（5.3.0 / 6.0.0，`MIT OR Apache-2.0 OR LGPL-2.1-or-later`）**：属多选一（`OR`）许可证，本项目严格依据其中 **`MIT OR Apache-2.0`** 条款选用，不触发 LGPL 义务。
2. **`epaint_default_fonts`（0.29.1，`(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0`）**：`egui` 默认内嵌字体资源（含 SIL Open Font License 1.1 与 Ubuntu Font Licence 1.0）。完整版（含 `gui` 特性）二进制发行时须随包保留本声明文件；纯无头构建（`--no-default-features`）不包含该 crate。
3. **`webpki-roots`（1.0.12，`CDLA-Permissive-2.0`）**：Mozilla 根证书信任数据，用于 `rustls` TLS 证书校验（宽松数据许可）。
4. **`Unicode-3.0`（18 个 ICU4X / IDNA 相关 crate）与 `Zlib` / `BSD-2-Clause` / `BSD-3-Clause` / `ISC` / `BSL-1.0` / `CC0-1.0` / `0BSD`**：均为宽松开源许可，发行时保留本声明即可满足版权与许可通知义务。

---

## 三、开发期设计参考库合规边界（不随二进制或发行包发布）

下列第三方项目曾作为**历史开发期**的前端排版与视觉设计本地参考。它们**不在当前公开源码树与发行包中**，不参与编译、不打包进任何二进制产物：

| 参考项目 | 来源 | 许可证 | 本项目合规边界 |
|---|---|---|---|
| **Axolotl Launcher** | `github.com/Mystic-Stars/Axolotl` | **GPL-3.0** | **代码、CSS、字体与素材零复制**；仅在开发期观摩视觉配色层次与组件间距思路，本项目全部采用自写 Rust/egui `theme.rs` 令牌实现，杜绝 GPL 传染。 |
| **System Monitor** | `github.com/realhugn/system-monitor` | 未见 LICENSE | **代码零复制**；仅观察多标签模块组织方式，无任何代码引入。 |
| **Dioxus** | `github.com/DioxusLabs/dioxus` | MIT / Apache-2.0 | 仅参考跨端一致布局理念，未引入框架或代码。 |
| **gpui-kit** | `github.com/longbridge/gpui-kit` | Apache-2.0 | 仅参考 DataTable / 语义主题设计范式，未引入框架或代码。 |
| **Council** | `github.com/declanmiddleton/Council` | MIT | 仅参考安全工具仪表盘分区气质，未复制代码。 |
| **The Grid** | `github.com/raaghulr/the_grid` | MIT | 仅参考高密度表格密度切换思路，未复制代码。 |
| **egui / egui_tiles** | `github.com/emilk/egui` | MIT / Apache-2.0 | 仅作 egui 官方示例参考；实际运行时依赖已列于下方 Cargo 依赖全表。 |
| **shadcn-admin** | 公开开源 demo 截图 | MIT | 仅截图参考整体骨架留白，无任何代码引入。 |

---

## 四、完整外部 Cargo 依赖清单（共 488 项，按包名与版本排序）

| 序号 | 包名（Crate） | 版本 | SPDX 许可证 | 仓库 / 主页 |
|---:|---|---|---|---|
| 1 | `ab_glyph` | `0.2.32` | `Apache-2.0` | https://github.com/alexheretic/ab-glyph |
| 2 | `ab_glyph_rasterizer` | `0.1.10` | `Apache-2.0` | https://github.com/alexheretic/ab-glyph |
| 3 | `accesskit` | `0.16.3` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 4 | `accesskit_atspi_common` | `0.9.3` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 5 | `accesskit_consumer` | `0.24.3` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 6 | `accesskit_macos` | `0.17.4` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 7 | `accesskit_unix` | `0.12.3` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 8 | `accesskit_windows` | `0.23.2` | `MIT OR Apache-2.0` | https://github.com/AccessKit/accesskit |
| 9 | `accesskit_winit` | `0.22.4` | `Apache-2.0` | https://github.com/AccessKit/accesskit |
| 10 | `adler2` | `2.0.1` | `0BSD OR MIT OR Apache-2.0` | https://github.com/oyvindln/adler2 |
| 11 | `ahash` | `0.8.12` | `MIT OR Apache-2.0` | https://github.com/tkaitchuck/ahash |
| 12 | `aho-corasick` | `1.1.5` | `Unlicense OR MIT` | https://github.com/BurntSushi/aho-corasick |
| 13 | `android-activity` | `0.6.1` | `MIT OR Apache-2.0` | https://github.com/rust-mobile/android-activity |
| 14 | `android-properties` | `0.2.2` | `MIT` | https://github.com/miklelappo/android-properties |
| 15 | `android_system_properties` | `0.1.6` | `MIT OR Apache-2.0` | https://github.com/nical/android_system_properties |
| 16 | `anstream` | `1.0.0` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 17 | `anstyle` | `1.0.14` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 18 | `anstyle-parse` | `1.0.0` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 19 | `anstyle-query` | `1.1.5` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 20 | `anstyle-wincon` | `3.0.11` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 21 | `anyhow` | `1.0.104` | `MIT OR Apache-2.0` | https://github.com/dtolnay/anyhow |
| 22 | `arboard` | `3.7.0` | `MIT OR Apache-2.0` | https://github.com/1Password/arboard |
| 23 | `arrayref` | `0.3.9` | `BSD-2-Clause` | https://github.com/droundy/arrayref |
| 24 | `arrayvec` | `0.7.8` | `MIT OR Apache-2.0` | https://github.com/bluss/arrayvec |
| 25 | `as-raw-xcb-connection` | `1.0.1` | `MIT OR Apache-2.0` | https://github.com/psychon/as-raw-xcb-connection |
| 26 | `ash` | `0.38.0+1.3.281` | `MIT OR Apache-2.0` | https://github.com/ash-rs/ash |
| 27 | `async-broadcast` | `0.7.2` | `MIT OR Apache-2.0` | https://github.com/smol-rs/async-broadcast |
| 28 | `async-channel` | `2.5.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-channel |
| 29 | `async-executor` | `1.14.1` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-executor |
| 30 | `async-fs` | `2.2.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-fs |
| 31 | `async-io` | `2.6.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-io |
| 32 | `async-lock` | `3.5.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-lock |
| 33 | `async-process` | `2.5.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-process |
| 34 | `async-recursion` | `1.2.0` | `MIT OR Apache-2.0` | https://github.com/dcchut/async-recursion |
| 35 | `async-signal` | `0.2.14` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-signal |
| 36 | `async-task` | `4.7.1` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/async-task |
| 37 | `async-trait` | `0.1.92` | `MIT OR Apache-2.0` | https://github.com/dtolnay/async-trait |
| 38 | `atomic-waker` | `1.1.2` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/atomic-waker |
| 39 | `atspi` | `0.22.0` | `Apache-2.0 OR MIT` | https://github.com/odilia-app/atspi |
| 40 | `atspi-common` | `0.6.0` | `Apache-2.0 OR MIT` | https://github.com/odilia-app/atspi |
| 41 | `atspi-connection` | `0.6.0` | `Apache-2.0 OR MIT` | https://github.com/odilia-app/atspi/ |
| 42 | `atspi-proxies` | `0.6.0` | `Apache-2.0 OR MIT` | https://github.com/odilia-app/atspi |
| 43 | `autocfg` | `1.5.1` | `Apache-2.0 OR MIT` | https://github.com/cuviper/autocfg |
| 44 | `base64` | `0.22.1` | `MIT OR Apache-2.0` | https://github.com/marshallpierce/rust-base64 |
| 45 | `base64` | `0.23.1` | `MIT OR Apache-2.0` | https://github.com/marshallpierce/rust-base64 |
| 46 | `bit-set` | `0.6.0` | `MIT/Apache-2.0` | https://github.com/contain-rs/bit-set |
| 47 | `bit-vec` | `0.7.0` | `MIT/Apache-2.0` | https://github.com/contain-rs/bit-vec |
| 48 | `bitflags` | `1.3.2` | `MIT/Apache-2.0` | https://github.com/bitflags/bitflags |
| 49 | `bitflags` | `2.13.2` | `MIT OR Apache-2.0` | https://github.com/bitflags/bitflags |
| 50 | `block` | `0.1.6` | `MIT` | http://github.com/SSheldon/rust-block |
| 51 | `block-buffer` | `0.10.4` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/utils |
| 52 | `block2` | `0.5.1` | `MIT` | https://github.com/madsmtm/objc2 |
| 53 | `blocking` | `1.7.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/blocking |
| 54 | `bumpalo` | `3.20.4` | `MIT OR Apache-2.0` | https://github.com/fitzgen/bumpalo |
| 55 | `bytemuck` | `1.25.2` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/Lokathor/bytemuck |
| 56 | `bytemuck_derive` | `1.12.1` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/Lokathor/bytemuck |
| 57 | `byteorder-lite` | `0.1.0` | `Unlicense OR MIT` | https://github.com/image-rs/byteorder-lite |
| 58 | `bytes` | `1.12.2` | `MIT` | https://github.com/tokio-rs/bytes |
| 59 | `calloop` | `0.13.0` | `MIT` | https://github.com/Smithay/calloop |
| 60 | `calloop` | `0.14.5` | `MIT` | https://github.com/Smithay/calloop |
| 61 | `calloop-wayland-source` | `0.3.0` | `MIT` | https://github.com/smithay/calloop-wayland-source |
| 62 | `calloop-wayland-source` | `0.4.1` | `MIT` | https://github.com/smithay/calloop-wayland-source |
| 63 | `cc` | `1.11.3` | `MIT OR Apache-2.0` | https://github.com/rust-lang/cc-rs |
| 64 | `cfg-if` | `1.0.5` | `MIT OR Apache-2.0` | https://github.com/rust-lang/cfg-if |
| 65 | `cfg_aliases` | `0.1.1` | `MIT` | https://github.com/katharostech/cfg_aliases |
| 66 | `cfg_aliases` | `0.2.2` | `MIT` | https://github.com/katharostech/cfg_aliases |
| 67 | `cgl` | `0.3.2` | `MIT / Apache-2.0` | https://github.com/servo/cgl-rs |
| 68 | `chacha20` | `0.10.2` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/stream-ciphers |
| 69 | `clap` | `4.6.9` | `MIT OR Apache-2.0` | https://github.com/clap-rs/clap |
| 70 | `clap_builder` | `4.6.7` | `MIT OR Apache-2.0` | https://github.com/clap-rs/clap |
| 71 | `clap_derive` | `4.6.9` | `MIT OR Apache-2.0` | https://github.com/clap-rs/clap |
| 72 | `clap_lex` | `1.1.2` | `MIT OR Apache-2.0` | https://github.com/clap-rs/clap |
| 73 | `clipboard-win` | `5.4.3` | `BSL-1.0` | https://github.com/DoumanAsh/clipboard-win |
| 74 | `codespan-reporting` | `0.11.1` | `Apache-2.0` | https://github.com/brendanzab/codespan |
| 75 | `colorchoice` | `1.0.5` | `MIT OR Apache-2.0` | https://github.com/rust-cli/anstyle.git |
| 76 | `com` | `0.6.0` | `MIT` | https://github.com/microsoft/com-rs |
| 77 | `com_macros` | `0.6.0` | `MIT` | https://github.com/microsoft/com-rs |
| 78 | `com_macros_support` | `0.6.0` | `MIT` | https://github.com/microsoft/com-rs |
| 79 | `combine` | `4.6.9` | `MIT` | https://github.com/Marwes/combine |
| 80 | `comfy-table` | `7.2.2` | `MIT` | https://github.com/nukesor/comfy-table |
| 81 | `concurrent-queue` | `2.5.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/concurrent-queue |
| 82 | `core-foundation` | `0.9.4` | `MIT OR Apache-2.0` | https://github.com/servo/core-foundation-rs |
| 83 | `core-foundation-sys` | `0.8.7` | `MIT OR Apache-2.0` | https://github.com/servo/core-foundation-rs |
| 84 | `core-graphics` | `0.23.2` | `MIT OR Apache-2.0` | https://github.com/servo/core-foundation-rs |
| 85 | `core-graphics-types` | `0.1.3` | `MIT OR Apache-2.0` | https://github.com/servo/core-foundation-rs |
| 86 | `core_detect` | `1.0.0` | `MIT/Apache-2.0` | https://github.com/thomcc/core_detect |
| 87 | `cpufeatures` | `0.2.17` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/utils |
| 88 | `cpufeatures` | `0.3.1` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/utils |
| 89 | `crc32fast` | `1.5.5` | `MIT OR Apache-2.0` | https://github.com/srijs/rust-crc32fast |
| 90 | `crossbeam-utils` | `0.8.23` | `MIT OR Apache-2.0` | https://github.com/crossbeam-rs/crossbeam |
| 91 | `crossterm` | `0.29.0` | `MIT` | https://github.com/crossterm-rs/crossterm |
| 92 | `crossterm_winapi` | `0.9.1` | `MIT` | https://github.com/crossterm-rs/crossterm-winapi |
| 93 | `crypto-common` | `0.1.7` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/traits |
| 94 | `cursor-icon` | `1.2.0` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/rust-windowing/cursor-icon |
| 95 | `deranged` | `0.5.9` | `MIT OR Apache-2.0` | https://github.com/jhpratt/deranged |
| 96 | `digest` | `0.10.7` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/traits |
| 97 | `dispatch` | `0.2.0` | `MIT` | http://github.com/SSheldon/rust-dispatch |
| 98 | `dispatch2` | `0.3.1` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/madsmtm/objc2 |
| 99 | `displaydoc` | `0.2.7` | `MIT OR Apache-2.0` | https://github.com/yaahc/displaydoc |
| 100 | `dlib` | `0.5.3` | `MIT` | https://github.com/elinorbgr/dlib |
| 101 | `document-features` | `0.2.12` | `MIT OR Apache-2.0` | https://github.com/slint-ui/document-features |
| 102 | `downcast-rs` | `1.2.1` | `MIT/Apache-2.0` | https://github.com/marcianx/downcast-rs |
| 103 | `dpi` | `0.1.2` | `Apache-2.0 AND MIT` | https://github.com/rust-windowing/winit |
| 104 | `ecolor` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui |
| 105 | `eframe` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/eframe |
| 106 | `egui` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui |
| 107 | `egui-wgpu` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/egui-wgpu |
| 108 | `egui-winit` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/egui-winit |
| 109 | `egui_glow` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/egui_glow |
| 110 | `emath` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/emath |
| 111 | `encoding_rs` | `0.8.44` | `(Apache-2.0 OR MIT) AND BSD-3-Clause` | https://github.com/hsivonen/encoding_rs |
| 112 | `endi` | `1.1.1` | `MIT` | https://github.com/zeenix/endi |
| 113 | `enumflags2` | `0.7.12` | `MIT OR Apache-2.0` | https://github.com/meithecatte/enumflags2 |
| 114 | `enumflags2_derive` | `0.7.12` | `MIT OR Apache-2.0` | https://github.com/meithecatte/enumflags2 |
| 115 | `epaint` | `0.29.1` | `MIT OR Apache-2.0` | https://github.com/emilk/egui/tree/master/crates/epaint |
| 116 | `epaint_default_fonts` | `0.29.1` | `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` | https://github.com/emilk/egui/tree/master/crates/epaint_default_fonts |
| 117 | `equivalent` | `1.0.3` | `Apache-2.0 OR MIT` | https://github.com/indexmap-rs/equivalent |
| 118 | `errno` | `0.3.15` | `MIT OR Apache-2.0` | https://github.com/lambda-fairy/rust-errno |
| 119 | `error-code` | `3.4.0` | `BSL-1.0` | https://github.com/DoumanAsh/error-code |
| 120 | `event-listener` | `5.4.2` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/event-listener |
| 121 | `event-listener-strategy` | `0.5.4` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/event-listener-strategy |
| 122 | `fastrand` | `2.5.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/fastrand |
| 123 | `fdeflate` | `0.3.7` | `MIT OR Apache-2.0` | https://github.com/image-rs/fdeflate |
| 124 | `find-msvc-tools` | `0.1.15` | `MIT OR Apache-2.0` | https://github.com/rust-lang/cc-rs |
| 125 | `flate2` | `1.1.11` | `MIT OR Apache-2.0` | https://github.com/rust-lang/flate2-rs |
| 126 | `fnv` | `1.0.7` | `Apache-2.0 / MIT` | https://github.com/servo/rust-fnv |
| 127 | `foldhash` | `0.1.5` | `Zlib` | https://github.com/orlp/foldhash |
| 128 | `foreign-types` | `0.5.0` | `MIT/Apache-2.0` | https://github.com/sfackler/foreign-types |
| 129 | `foreign-types-macros` | `0.2.4` | `MIT/Apache-2.0` | https://github.com/sfackler/foreign-types |
| 130 | `foreign-types-shared` | `0.3.1` | `MIT/Apache-2.0` | https://github.com/sfackler/foreign-types |
| 131 | `form_urlencoded` | `1.2.3` | `MIT OR Apache-2.0` | https://github.com/servo/rust-url |
| 132 | `fs2` | `0.4.3` | `MIT/Apache-2.0` | https://github.com/danburkert/fs2-rs |
| 133 | `futures-channel` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 134 | `futures-core` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 135 | `futures-io` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 136 | `futures-lite` | `2.6.1` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/futures-lite |
| 137 | `futures-macro` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 138 | `futures-sink` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 139 | `futures-task` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 140 | `futures-util` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/futures-rs |
| 141 | `generic-array` | `0.14.7` | `MIT` | https://github.com/fizyk20/generic-array.git |
| 142 | `gethostname` | `1.1.0` | `Apache-2.0` | https://codeberg.org/swsnr/gethostname.rs.git |
| 143 | `getrandom` | `0.2.17` | `MIT OR Apache-2.0` | https://github.com/rust-random/getrandom |
| 144 | `getrandom` | `0.3.4` | `MIT OR Apache-2.0` | https://github.com/rust-random/getrandom |
| 145 | `getrandom` | `0.4.3` | `MIT OR Apache-2.0` | https://github.com/rust-random/getrandom |
| 146 | `gl_generator` | `0.14.0` | `Apache-2.0` | https://github.com/brendanzab/gl-rs/ |
| 147 | `glow` | `0.13.1` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/grovesNL/glow |
| 148 | `glow` | `0.14.2` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/grovesNL/glow |
| 149 | `glutin` | `0.32.3` | `Apache-2.0` | https://github.com/rust-windowing/glutin |
| 150 | `glutin-winit` | `0.5.0` | `MIT` | https://github.com/rust-windowing/glutin |
| 151 | `glutin_egl_sys` | `0.7.1` | `Apache-2.0` | https://github.com/rust-windowing/glutin |
| 152 | `glutin_glx_sys` | `0.6.1` | `Apache-2.0` | https://github.com/rust-windowing/glutin |
| 153 | `glutin_wgl_sys` | `0.6.1` | `Apache-2.0` | https://github.com/rust-windowing/glutin |
| 154 | `gpu-alloc` | `0.6.2` | `MIT OR Apache-2.0` | https://github.com/zakarumych/gpu-alloc |
| 155 | `gpu-alloc-types` | `0.3.1` | `MIT OR Apache-2.0` | https://github.com/zakarumych/gpu-alloc |
| 156 | `gpu-allocator` | `0.26.0` | `MIT OR Apache-2.0` | https://github.com/Traverse-Research/gpu-allocator |
| 157 | `gpu-descriptor` | `0.3.2` | `MIT OR Apache-2.0` | https://github.com/zakarumych/gpu-descriptor |
| 158 | `gpu-descriptor-types` | `0.2.0` | `MIT OR Apache-2.0` | https://github.com/zakarumych/gpu-descriptor |
| 159 | `h2` | `0.4.20` | `MIT` | https://github.com/hyperium/h2 |
| 160 | `hashbrown` | `0.15.5` | `MIT OR Apache-2.0` | https://github.com/rust-lang/hashbrown |
| 161 | `hashbrown` | `0.17.1` | `MIT OR Apache-2.0` | https://github.com/rust-lang/hashbrown |
| 162 | `hassle-rs` | `0.11.0` | `MIT` | https://github.com/Traverse-Research/hassle-rs |
| 163 | `heck` | `0.5.0` | `MIT OR Apache-2.0` | https://github.com/withoutboats/heck |
| 164 | `hermit-abi` | `0.5.3` | `MIT OR Apache-2.0` | https://github.com/hermit-os/hermit-rs |
| 165 | `hex` | `0.4.3` | `MIT OR Apache-2.0` | https://github.com/KokaKiwi/rust-hex |
| 166 | `hexf-parse` | `0.2.1` | `CC0-1.0` | https://github.com/lifthrasiir/hexf |
| 167 | `http` | `1.5.0` | `MIT OR Apache-2.0` | https://github.com/hyperium/http |
| 168 | `http-body` | `1.1.0` | `MIT` | https://github.com/hyperium/http-body |
| 169 | `http-body-util` | `0.1.5` | `MIT` | https://github.com/hyperium/http-body |
| 170 | `httparse` | `1.10.1` | `MIT OR Apache-2.0` | https://github.com/seanmonstar/httparse |
| 171 | `hyper` | `1.12.0` | `MIT` | https://github.com/hyperium/hyper |
| 172 | `hyper-rustls` | `0.27.10` | `Apache-2.0 OR ISC OR MIT` | https://github.com/rustls/hyper-rustls |
| 173 | `hyper-util` | `0.1.21` | `MIT` | https://github.com/hyperium/hyper-util |
| 174 | `icu_collections` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 175 | `icu_locale_core` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 176 | `icu_normalizer` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 177 | `icu_normalizer_data` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 178 | `icu_properties` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 179 | `icu_properties_data` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 180 | `icu_provider` | `2.4.0` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 181 | `idna` | `1.1.0` | `MIT OR Apache-2.0` | https://github.com/servo/rust-url/ |
| 182 | `idna_adapter` | `1.2.2` | `Apache-2.0 OR MIT` | https://github.com/hsivonen/idna_adapter |
| 183 | `if-addrs` | `0.13.4` | `MIT OR BSD-3-Clause` | https://github.com/messense/if-addrs |
| 184 | `image` | `0.25.10` | `MIT OR Apache-2.0` | https://github.com/image-rs/image |
| 185 | `immutable-chunkmap` | `2.1.5` | `Apache-2.0 OR MIT` | https://github.com/estokes/immutable-chunkmap |
| 186 | `indexmap` | `2.14.2` | `Apache-2.0 OR MIT` | https://github.com/indexmap-rs/indexmap |
| 187 | `ipnet` | `2.12.2` | `MIT OR Apache-2.0` | https://github.com/krisprice/ipnet |
| 188 | `is_terminal_polyfill` | `1.70.2` | `MIT OR Apache-2.0` | https://github.com/polyfill-rs/is_terminal_polyfill |
| 189 | `itoa` | `1.0.19` | `MIT OR Apache-2.0` | https://github.com/dtolnay/itoa |
| 190 | `jni` | `0.22.4` | `MIT OR Apache-2.0` | https://github.com/jni-rs/jni-rs |
| 191 | `jni-macros` | `0.22.4` | `MIT OR Apache-2.0` | https://github.com/jni-rs/jni-rs |
| 192 | `jni-sys` | `0.3.1` | `MIT OR Apache-2.0` | https://github.com/jni-rs/jni-sys |
| 193 | `jni-sys` | `0.4.1` | `MIT OR Apache-2.0` | https://github.com/jni-rs/jni-sys |
| 194 | `jni-sys-macros` | `0.4.1` | `MIT OR Apache-2.0` | https://github.com/jni-rs/jni-sys |
| 195 | `jobserver` | `0.1.35` | `MIT OR Apache-2.0` | https://github.com/rust-lang/jobserver-rs |
| 196 | `js-sys` | `0.3.107` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys |
| 197 | `khronos-egl` | `6.0.0` | `MIT/Apache-2.0` | https://github.com/timothee-haudebourg/khronos-egl |
| 198 | `khronos_api` | `3.1.0` | `Apache-2.0` | https://github.com/brendanzab/gl-rs/ |
| 199 | `libc` | `0.2.191` | `MIT OR Apache-2.0` | https://github.com/rust-lang/libc |
| 200 | `libloading` | `0.8.9` | `ISC` | https://github.com/nagisa/rust_libloading/ |
| 201 | `libredox` | `0.1.25` | `MIT` | https://gitlab.redox-os.org/redox-os/libredox.git |
| 202 | `linux-raw-sys` | `0.12.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/sunfishcode/linux-raw-sys |
| 203 | `linux-raw-sys` | `0.4.15` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/sunfishcode/linux-raw-sys |
| 204 | `litemap` | `0.8.3` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 205 | `litrs` | `1.0.1` | `MIT OR Apache-2.0` | https://github.com/LukasKalbertodt/litrs |
| 206 | `lock_api` | `0.4.14` | `MIT OR Apache-2.0` | https://github.com/Amanieu/parking_lot |
| 207 | `log` | `0.4.35` | `MIT OR Apache-2.0` | https://github.com/rust-lang/log |
| 208 | `lru-slab` | `0.1.3` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/Ralith/lru-slab |
| 209 | `malloc_buf` | `0.0.6` | `MIT` | https://github.com/SSheldon/malloc_buf |
| 210 | `memchr` | `2.8.3` | `Unlicense OR MIT` | https://github.com/BurntSushi/memchr |
| 211 | `memmap2` | `0.9.12` | `MIT OR Apache-2.0` | https://github.com/RazrFalcon/memmap2-rs |
| 212 | `memoffset` | `0.9.1` | `MIT` | https://github.com/Gilnaa/memoffset |
| 213 | `metal` | `0.29.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/metal-rs |
| 214 | `mime` | `0.3.18` | `MIT OR Apache-2.0` | https://github.com/hyperium/mime |
| 215 | `miniz_oxide` | `0.8.9` | `MIT OR Zlib OR Apache-2.0` | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| 216 | `miniz_oxide` | `0.9.1` | `MIT OR Zlib OR Apache-2.0` | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| 217 | `mio` | `1.2.5` | `MIT` | https://github.com/tokio-rs/mio |
| 218 | `moxcms` | `0.8.1` | `BSD-3-Clause OR Apache-2.0` | https://github.com/awxkee/moxcms.git |
| 219 | `multiversion_no_op` | `1.0.0` | `Apache-2.0 OR MIT` | https://github.com/hsivonen/multiversion_no_op |
| 220 | `naga` | `22.1.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/wgpu/tree/trunk/naga |
| 221 | `ndk` | `0.9.0` | `MIT OR Apache-2.0` | https://github.com/rust-mobile/ndk |
| 222 | `ndk-context` | `0.1.1` | `MIT OR Apache-2.0` | https://github.com/rust-windowing/android-ndk-rs |
| 223 | `ndk-sys` | `0.5.0+25.2.9519653` | `MIT OR Apache-2.0` | https://github.com/rust-mobile/ndk |
| 224 | `ndk-sys` | `0.6.0+11769913` | `MIT OR Apache-2.0` | https://github.com/rust-mobile/ndk |
| 225 | `nix` | `0.29.0` | `MIT` | https://github.com/nix-rust/nix |
| 226 | `nohash-hasher` | `0.2.0` | `Apache-2.0 OR MIT` | https://github.com/paritytech/nohash-hasher |
| 227 | `num-conv` | `0.2.2` | `MIT OR Apache-2.0` | https://github.com/jhpratt/num-conv |
| 228 | `num-traits` | `0.2.19` | `MIT OR Apache-2.0` | https://github.com/rust-num/num-traits |
| 229 | `num_enum` | `0.7.6` | `BSD-3-Clause OR MIT OR Apache-2.0` | https://github.com/illicitonion/num_enum |
| 230 | `num_enum_derive` | `0.7.6` | `BSD-3-Clause OR MIT OR Apache-2.0` | https://github.com/illicitonion/num_enum |
| 231 | `objc` | `0.2.7` | `MIT` | http://github.com/SSheldon/rust-objc |
| 232 | `objc-sys` | `0.3.5` | `MIT` | https://github.com/madsmtm/objc2 |
| 233 | `objc2` | `0.5.3` | `MIT` | https://github.com/madsmtm/objc2 |
| 234 | `objc2` | `0.6.5` | `MIT` | https://github.com/madsmtm/objc2 |
| 235 | `objc2-app-kit` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 236 | `objc2-app-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/madsmtm/objc2 |
| 237 | `objc2-cloud-kit` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 238 | `objc2-contacts` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 239 | `objc2-core-data` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 240 | `objc2-core-foundation` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/madsmtm/objc2 |
| 241 | `objc2-core-graphics` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/madsmtm/objc2 |
| 242 | `objc2-core-image` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 243 | `objc2-core-location` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 244 | `objc2-encode` | `4.1.0` | `MIT` | https://github.com/madsmtm/objc2 |
| 245 | `objc2-foundation` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 246 | `objc2-foundation` | `0.3.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 247 | `objc2-io-surface` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/madsmtm/objc2 |
| 248 | `objc2-link-presentation` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 249 | `objc2-metal` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 250 | `objc2-quartz-core` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 251 | `objc2-symbols` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 252 | `objc2-ui-kit` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 253 | `objc2-uniform-type-identifiers` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 254 | `objc2-user-notifications` | `0.2.2` | `MIT` | https://github.com/madsmtm/objc2 |
| 255 | `once_cell` | `1.21.4` | `MIT OR Apache-2.0` | https://github.com/matklad/once_cell |
| 256 | `once_cell_polyfill` | `1.70.2` | `MIT OR Apache-2.0` | https://github.com/polyfill-rs/once_cell_polyfill |
| 257 | `orbclient` | `0.3.55` | `MIT` | https://gitlab.redox-os.org/redox-os/orbclient |
| 258 | `ordered-stream` | `0.2.0` | `MIT OR Apache-2.0` | https://github.com/danieldg/ordered-stream |
| 259 | `owned_ttf_parser` | `0.25.1` | `Apache-2.0` | https://github.com/alexheretic/owned-ttf-parser |
| 260 | `parking` | `2.2.1` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/parking |
| 261 | `parking_lot` | `0.12.5` | `MIT OR Apache-2.0` | https://github.com/Amanieu/parking_lot |
| 262 | `parking_lot_core` | `0.9.12` | `MIT OR Apache-2.0` | https://github.com/Amanieu/parking_lot |
| 263 | `paste` | `1.0.15` | `MIT OR Apache-2.0` | https://github.com/dtolnay/paste |
| 264 | `percent-encoding` | `2.3.2` | `MIT OR Apache-2.0` | https://github.com/servo/rust-url/ |
| 265 | `pin-project` | `1.2.0` | `Apache-2.0 OR MIT` | https://github.com/taiki-e/pin-project |
| 266 | `pin-project-internal` | `1.2.0` | `Apache-2.0 OR MIT` | https://github.com/taiki-e/pin-project |
| 267 | `pin-project-lite` | `0.2.18` | `Apache-2.0 OR MIT` | https://github.com/taiki-e/pin-project-lite |
| 268 | `piper` | `0.2.5` | `MIT OR Apache-2.0` | https://github.com/smol-rs/piper |
| 269 | `pkg-config` | `0.3.34` | `MIT OR Apache-2.0` | https://github.com/rust-lang/pkg-config-rs |
| 270 | `plain` | `0.2.3` | `MIT/Apache-2.0` | https://github.com/randomites/plain |
| 271 | `png` | `0.18.1` | `MIT OR Apache-2.0` | https://github.com/image-rs/image-png |
| 272 | `polling` | `3.11.0` | `Apache-2.0 OR MIT` | https://github.com/smol-rs/polling |
| 273 | `potential_utf` | `0.1.6` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 274 | `powerfmt` | `0.2.1` | `MIT OR Apache-2.0` | https://github.com/jhpratt/powerfmt |
| 275 | `ppv-lite86` | `0.2.21` | `MIT OR Apache-2.0` | https://github.com/cryptocorrosion/cryptocorrosion |
| 276 | `presser` | `0.3.1` | `MIT OR Apache-2.0` | https://github.com/EmbarkStudios/presser |
| 277 | `proc-macro-crate` | `3.5.0` | `MIT OR Apache-2.0` | https://github.com/bkchr/proc-macro-crate |
| 278 | `proc-macro2` | `1.0.107` | `MIT OR Apache-2.0` | https://github.com/dtolnay/proc-macro2 |
| 279 | `profiling` | `1.0.18` | `MIT OR Apache-2.0` | https://github.com/aclysma/profiling |
| 280 | `pxfm` | `0.1.30` | `BSD-3-Clause OR Apache-2.0` | https://github.com/awxkee/pxfm |
| 281 | `quick-xml` | `0.30.0` | `MIT` | https://github.com/tafia/quick-xml |
| 282 | `quick-xml` | `0.41.0` | `MIT` | https://github.com/tafia/quick-xml |
| 283 | `quinn` | `0.11.12` | `MIT OR Apache-2.0` | https://github.com/quinn-rs/quinn |
| 284 | `quinn-proto` | `0.11.19` | `MIT OR Apache-2.0` | https://github.com/quinn-rs/quinn |
| 285 | `quinn-udp` | `0.5.16` | `MIT OR Apache-2.0` | https://github.com/quinn-rs/quinn |
| 286 | `quote` | `1.0.47` | `MIT OR Apache-2.0` | https://github.com/dtolnay/quote |
| 287 | `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | https://github.com/r-efi/r-efi |
| 288 | `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | https://github.com/r-efi/r-efi |
| 289 | `rand` | `0.10.4` | `MIT OR Apache-2.0` | https://github.com/rust-random/rand |
| 290 | `rand` | `0.8.8` | `MIT OR Apache-2.0` | https://github.com/rust-random/rand |
| 291 | `rand_chacha` | `0.3.1` | `MIT OR Apache-2.0` | https://github.com/rust-random/rand |
| 292 | `rand_core` | `0.10.1` | `MIT OR Apache-2.0` | https://github.com/rust-random/rand_core |
| 293 | `rand_core` | `0.6.4` | `MIT OR Apache-2.0` | https://github.com/rust-random/rand |
| 294 | `rand_pcg` | `0.10.3` | `MIT OR Apache-2.0` | https://github.com/rust-random/rngs |
| 295 | `raw-window-handle` | `0.6.2` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/rust-windowing/raw-window-handle |
| 296 | `redox_syscall` | `0.4.1` | `MIT` | https://gitlab.redox-os.org/redox-os/syscall |
| 297 | `redox_syscall` | `0.5.18` | `MIT` | https://gitlab.redox-os.org/redox-os/syscall |
| 298 | `redox_syscall` | `0.9.5` | `MIT` | https://gitlab.redox-os.org/redox-os/kernel |
| 299 | `regex` | `1.13.2` | `MIT OR Apache-2.0` | https://github.com/rust-lang/regex |
| 300 | `regex-automata` | `0.4.19` | `MIT OR Apache-2.0` | https://github.com/rust-lang/regex |
| 301 | `regex-syntax` | `0.8.12` | `MIT OR Apache-2.0` | https://github.com/rust-lang/regex |
| 302 | `renderdoc-sys` | `1.1.0` | `MIT OR Apache-2.0` | https://github.com/ebkalderon/renderdoc-rs |
| 303 | `reqwest` | `0.12.28` | `MIT OR Apache-2.0` | https://github.com/seanmonstar/reqwest |
| 304 | `ring` | `0.17.14` | `Apache-2.0 AND ISC` | https://github.com/briansmith/ring |
| 305 | `rustc-hash` | `1.1.0` | `Apache-2.0/MIT` | https://github.com/rust-lang-nursery/rustc-hash |
| 306 | `rustc-hash` | `2.1.3` | `Apache-2.0 OR MIT` | https://github.com/rust-lang/rustc-hash |
| 307 | `rustc_version` | `0.4.1` | `MIT OR Apache-2.0` | https://github.com/djc/rustc-version-rs |
| 308 | `rustix` | `0.38.44` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/bytecodealliance/rustix |
| 309 | `rustix` | `1.1.6` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/bytecodealliance/rustix |
| 310 | `rustls` | `0.23.46` | `Apache-2.0 OR ISC OR MIT` | https://github.com/rustls/rustls |
| 311 | `rustls-pki-types` | `1.16.0` | `MIT OR Apache-2.0` | https://github.com/rustls/pki-types |
| 312 | `rustls-webpki` | `0.103.17` | `ISC` | https://github.com/rustls/webpki |
| 313 | `rustversion` | `1.0.23` | `MIT OR Apache-2.0` | https://github.com/dtolnay/rustversion |
| 314 | `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` | https://github.com/dtolnay/ryu |
| 315 | `same-file` | `1.0.6` | `Unlicense/MIT` | https://github.com/BurntSushi/same-file |
| 316 | `scoped-tls` | `1.0.1` | `MIT/Apache-2.0` | https://github.com/alexcrichton/scoped-tls |
| 317 | `scopeguard` | `1.2.0` | `MIT OR Apache-2.0` | https://github.com/bluss/scopeguard |
| 318 | `sctk-adwaita` | `0.10.1` | `MIT` | https://github.com/PolyMeilex/sctk-adwaita |
| 319 | `semver` | `1.0.28` | `MIT OR Apache-2.0` | https://github.com/dtolnay/semver |
| 320 | `serde` | `1.0.229` | `MIT OR Apache-2.0` | https://github.com/serde-rs/serde |
| 321 | `serde_core` | `1.0.229` | `MIT OR Apache-2.0` | https://github.com/serde-rs/serde |
| 322 | `serde_derive` | `1.0.229` | `MIT OR Apache-2.0` | https://github.com/serde-rs/serde |
| 323 | `serde_json` | `1.0.154` | `MIT OR Apache-2.0` | https://github.com/serde-rs/json |
| 324 | `serde_repr` | `0.1.21` | `MIT OR Apache-2.0` | https://github.com/dtolnay/serde-repr |
| 325 | `serde_spanned` | `0.6.9` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 326 | `serde_urlencoded` | `0.7.1` | `MIT/Apache-2.0` | https://github.com/nox/serde_urlencoded |
| 327 | `sha1` | `0.10.7` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/hashes |
| 328 | `sha2` | `0.10.9` | `MIT OR Apache-2.0` | https://github.com/RustCrypto/hashes |
| 329 | `shlex` | `2.0.1` | `MIT OR Apache-2.0` | https://github.com/comex/rust-shlex |
| 330 | `signal-hook-registry` | `1.4.8` | `MIT OR Apache-2.0` | https://github.com/vorner/signal-hook |
| 331 | `simd-adler32` | `0.3.10` | `MIT` | https://github.com/mcountryman/simd-adler32 |
| 332 | `simd_cesu8` | `1.2.0` | `Apache-2.0 OR MIT` | https://github.com/seancroach/simd_cesu8 |
| 333 | `simdutf8` | `0.1.5` | `MIT OR Apache-2.0` | https://github.com/rusticstuff/simdutf8 |
| 334 | `slab` | `0.4.12` | `MIT` | https://github.com/tokio-rs/slab |
| 335 | `slotmap` | `1.1.1` | `Zlib` | https://github.com/orlp/slotmap |
| 336 | `smallvec` | `1.16.6` | `MIT OR Apache-2.0` | https://github.com/smallvec/smallvec |
| 337 | `smithay-client-toolkit` | `0.19.2` | `MIT` | https://github.com/smithay/client-toolkit |
| 338 | `smithay-client-toolkit` | `0.20.0` | `MIT` | https://github.com/smithay/client-toolkit |
| 339 | `smithay-clipboard` | `0.7.3` | `MIT` | https://github.com/smithay/smithay-clipboard |
| 340 | `smol_str` | `0.2.2` | `MIT OR Apache-2.0` | https://github.com/rust-analyzer/smol_str |
| 341 | `socket2` | `0.6.5` | `MIT OR Apache-2.0` | https://github.com/rust-lang/socket2 |
| 342 | `spirv` | `0.3.0+sdk-1.3.268.0` | `Apache-2.0` | https://github.com/gfx-rs/rspirv |
| 343 | `stable_deref_trait` | `1.2.1` | `MIT OR Apache-2.0` | https://github.com/storyyeller/stable_deref_trait |
| 344 | `static_assertions` | `1.1.0` | `MIT OR Apache-2.0` | https://github.com/nvzqz/static-assertions-rs |
| 345 | `strict-num` | `0.1.1` | `MIT` | https://github.com/RazrFalcon/strict-num |
| 346 | `strsim` | `0.11.1` | `MIT` | https://github.com/rapidfuzz/strsim-rs |
| 347 | `subtle` | `2.6.1` | `BSD-3-Clause` | https://github.com/dalek-cryptography/subtle |
| 348 | `syn` | `1.0.109` | `MIT OR Apache-2.0` | https://github.com/dtolnay/syn |
| 349 | `syn` | `2.0.119` | `MIT OR Apache-2.0` | https://github.com/dtolnay/syn |
| 350 | `syn` | `3.0.10` | `MIT OR Apache-2.0` | https://github.com/dtolnay/syn |
| 351 | `sync_wrapper` | `1.0.2` | `Apache-2.0` | https://github.com/Actyx/sync_wrapper |
| 352 | `synstructure` | `0.14.0` | `MIT` | https://github.com/mystor/synstructure |
| 353 | `tempfile` | `3.27.0` | `MIT OR Apache-2.0` | https://github.com/Stebalien/tempfile |
| 354 | `termcolor` | `1.4.1` | `Unlicense OR MIT` | https://github.com/BurntSushi/termcolor |
| 355 | `terminal_size` | `0.4.4` | `MIT OR Apache-2.0` | https://github.com/eminence/terminal-size |
| 356 | `thiserror` | `1.0.69` | `MIT OR Apache-2.0` | https://github.com/dtolnay/thiserror |
| 357 | `thiserror` | `2.0.21` | `MIT OR Apache-2.0` | https://github.com/dtolnay/thiserror |
| 358 | `thiserror-impl` | `1.0.69` | `MIT OR Apache-2.0` | https://github.com/dtolnay/thiserror |
| 359 | `thiserror-impl` | `2.0.21` | `MIT OR Apache-2.0` | https://github.com/dtolnay/thiserror |
| 360 | `time` | `0.3.55` | `MIT OR Apache-2.0` | https://github.com/time-rs/time |
| 361 | `time-core` | `0.1.9` | `MIT OR Apache-2.0` | https://github.com/time-rs/time |
| 362 | `time-macros` | `0.2.32` | `MIT OR Apache-2.0` | https://github.com/time-rs/time |
| 363 | `tiny-skia` | `0.11.4` | `BSD-3-Clause` | https://github.com/RazrFalcon/tiny-skia |
| 364 | `tiny-skia-path` | `0.11.4` | `BSD-3-Clause` | https://github.com/RazrFalcon/tiny-skia/tree/master/path |
| 365 | `tinystr` | `0.8.5` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 366 | `tinyvec` | `1.13.3` | `Zlib OR Apache-2.0 OR MIT` | https://github.com/Lokathor/tinyvec |
| 367 | `tokio` | `1.54.1` | `MIT` | https://github.com/tokio-rs/tokio |
| 368 | `tokio-macros` | `2.7.2` | `MIT` | https://github.com/tokio-rs/tokio |
| 369 | `tokio-rustls` | `0.26.6` | `MIT OR Apache-2.0` | https://github.com/rustls/tokio-rustls |
| 370 | `tokio-util` | `0.7.20` | `MIT` | https://github.com/tokio-rs/tokio |
| 371 | `toml` | `0.8.23` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 372 | `toml_datetime` | `0.6.11` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 373 | `toml_datetime` | `1.1.3+spec-1.1.0` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 374 | `toml_edit` | `0.22.27` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 375 | `toml_edit` | `0.25.20+spec-1.1.0` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 376 | `toml_parser` | `1.1.6+spec-1.1.0` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 377 | `toml_write` | `0.1.2` | `MIT OR Apache-2.0` | https://github.com/toml-rs/toml |
| 378 | `tower` | `0.5.3` | `MIT` | https://github.com/tower-rs/tower |
| 379 | `tower-http` | `0.6.11` | `MIT` | https://github.com/tower-rs/tower-http |
| 380 | `tower-layer` | `0.3.3` | `MIT` | https://github.com/tower-rs/tower |
| 381 | `tower-service` | `0.3.3` | `MIT` | https://github.com/tower-rs/tower |
| 382 | `tracing` | `0.1.44` | `MIT` | https://github.com/tokio-rs/tracing |
| 383 | `tracing-attributes` | `0.1.31` | `MIT` | https://github.com/tokio-rs/tracing |
| 384 | `tracing-core` | `0.1.36` | `MIT` | https://github.com/tokio-rs/tracing |
| 385 | `try-lock` | `0.2.5` | `MIT` | https://github.com/seanmonstar/try-lock |
| 386 | `ttf-parser` | `0.25.1` | `MIT OR Apache-2.0` | https://github.com/harfbuzz/ttf-parser |
| 387 | `type-map` | `0.5.1` | `MIT/Apache-2.0` | https://github.com/kardeiz/type-map |
| 388 | `typenum` | `1.20.1` | `MIT OR Apache-2.0` | https://github.com/paholg/typenum |
| 389 | `uds_windows` | `1.2.1` | `MIT` | https://github.com/haraldh/rust_uds_windows |
| 390 | `unicode-ident` | `1.0.27` | `(MIT OR Apache-2.0) AND Unicode-3.0` | https://github.com/dtolnay/unicode-ident |
| 391 | `unicode-segmentation` | `1.13.3` | `MIT OR Apache-2.0` | https://github.com/unicode-rs/unicode-segmentation |
| 392 | `unicode-width` | `0.1.14` | `MIT OR Apache-2.0` | https://github.com/unicode-rs/unicode-width |
| 393 | `unicode-width` | `0.2.3` | `MIT OR Apache-2.0` | https://github.com/unicode-rs/unicode-width |
| 394 | `unicode-xid` | `0.2.6` | `MIT OR Apache-2.0` | https://github.com/unicode-rs/unicode-xid |
| 395 | `untrusted` | `0.9.0` | `ISC` | https://github.com/briansmith/untrusted |
| 396 | `url` | `2.5.9` | `MIT OR Apache-2.0` | https://github.com/servo/rust-url |
| 397 | `utf8_iter` | `1.1.0` | `Apache-2.0 OR MIT` | https://github.com/hsivonen/utf8_iter |
| 398 | `utf8parse` | `0.2.2` | `Apache-2.0 OR MIT` | https://github.com/alacritty/vte |
| 399 | `utf_types` | `1.0.0` | `Apache-2.0 OR MIT` | https://github.com/hsivonen/utf_types |
| 400 | `version_check` | `0.9.5` | `MIT/Apache-2.0` | https://github.com/SergioBenitez/version_check |
| 401 | `walkdir` | `2.5.1` | `Unlicense/MIT` | https://github.com/BurntSushi/walkdir |
| 402 | `want` | `0.3.2` | `MIT` | https://github.com/seanmonstar/want |
| 403 | `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/bytecodealliance/wasi |
| 404 | `wasip2` | `1.0.4+wasi-0.2.12` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/bytecodealliance/wasi-rs |
| 405 | `wasm-bindgen` | `0.2.130` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen |
| 406 | `wasm-bindgen-futures` | `0.4.80` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures |
| 407 | `wasm-bindgen-macro` | `0.2.130` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro |
| 408 | `wasm-bindgen-macro-support` | `0.2.130` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support |
| 409 | `wasm-bindgen-shared` | `0.2.130` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared |
| 410 | `wayland-backend` | `0.3.17` | `MIT` | https://github.com/smithay/wayland-rs |
| 411 | `wayland-client` | `0.31.15` | `MIT` | https://github.com/smithay/wayland-rs |
| 412 | `wayland-csd-frame` | `0.3.0` | `MIT` | https://github.com/rust-windowing/wayland-csd-frame |
| 413 | `wayland-cursor` | `0.31.14` | `MIT` | https://github.com/smithay/wayland-rs |
| 414 | `wayland-protocols` | `0.32.13` | `MIT` | https://github.com/smithay/wayland-rs |
| 415 | `wayland-protocols-experimental` | `20250721.0.1` | `MIT` | https://github.com/smithay/wayland-rs |
| 416 | `wayland-protocols-misc` | `0.3.12` | `MIT` | https://github.com/smithay/wayland-rs |
| 417 | `wayland-protocols-plasma` | `0.3.12` | `MIT` | https://github.com/smithay/wayland-rs |
| 418 | `wayland-protocols-wlr` | `0.3.12` | `MIT` | https://github.com/smithay/wayland-rs |
| 419 | `wayland-scanner` | `0.31.11` | `MIT` | https://github.com/smithay/wayland-rs |
| 420 | `wayland-sys` | `0.31.11` | `MIT` | https://github.com/smithay/wayland-rs |
| 421 | `web-sys` | `0.3.107` | `MIT OR Apache-2.0` | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys |
| 422 | `web-time` | `1.1.0` | `MIT OR Apache-2.0` | https://github.com/daxpedda/web-time |
| 423 | `webbrowser` | `1.2.7` | `MIT OR Apache-2.0` | https://github.com/amodm/webbrowser-rs |
| 424 | `webpki-roots` | `1.0.12` | `CDLA-Permissive-2.0` | https://github.com/rustls/webpki-roots |
| 425 | `wgpu` | `22.1.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/wgpu |
| 426 | `wgpu-core` | `22.1.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/wgpu |
| 427 | `wgpu-hal` | `22.0.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/wgpu |
| 428 | `wgpu-types` | `22.0.0` | `MIT OR Apache-2.0` | https://github.com/gfx-rs/wgpu |
| 429 | `widestring` | `1.2.1` | `MIT OR Apache-2.0` | https://github.com/VoidStarKat/widestring-rs |
| 430 | `winapi` | `0.3.9` | `MIT/Apache-2.0` | https://github.com/retep998/winapi-rs |
| 431 | `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | https://github.com/retep998/winapi-rs |
| 432 | `winapi-util` | `0.1.11` | `Unlicense OR MIT` | https://github.com/BurntSushi/winapi-util |
| 433 | `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | https://github.com/retep998/winapi-rs |
| 434 | `windows` | `0.52.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 435 | `windows` | `0.58.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 436 | `windows-core` | `0.52.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 437 | `windows-core` | `0.58.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 438 | `windows-implement` | `0.58.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 439 | `windows-interface` | `0.58.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 440 | `windows-link` | `0.2.1` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 441 | `windows-result` | `0.2.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 442 | `windows-strings` | `0.1.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 443 | `windows-sys` | `0.52.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 444 | `windows-sys` | `0.59.0` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 445 | `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 446 | `windows-targets` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 447 | `windows_aarch64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 448 | `windows_aarch64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 449 | `windows_i686_gnu` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 450 | `windows_i686_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 451 | `windows_i686_msvc` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 452 | `windows_x86_64_gnu` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 453 | `windows_x86_64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 454 | `windows_x86_64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | https://github.com/microsoft/windows-rs |
| 455 | `winit` | `0.30.13` | `Apache-2.0` | https://github.com/rust-windowing/winit |
| 456 | `winnow` | `0.7.15` | `MIT` | https://github.com/winnow-rs/winnow |
| 457 | `winnow` | `1.0.5` | `MIT` | https://github.com/winnow-rs/winnow |
| 458 | `wit-bindgen` | `0.57.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | https://github.com/bytecodealliance/wit-bindgen |
| 459 | `writeable` | `0.6.4` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 460 | `x11-dl` | `2.21.0` | `MIT` | https://github.com/AltF02/x11-rs.git |
| 461 | `x11rb` | `0.13.2` | `MIT OR Apache-2.0` | https://github.com/psychon/x11rb |
| 462 | `x11rb-protocol` | `0.13.2` | `MIT OR Apache-2.0` | https://github.com/psychon/x11rb |
| 463 | `xcursor` | `0.3.11` | `MIT` | https://github.com/esposm03/xcursor-rs |
| 464 | `xdg-home` | `1.3.0` | `MIT` | https://github.com/zeenix/xdg-home |
| 465 | `xkbcommon-dl` | `0.4.2` | `MIT` | https://github.com/rust-windowing/xkbcommon-dl |
| 466 | `xkeysym` | `0.2.1` | `MIT OR Apache-2.0 OR Zlib` | https://github.com/notgull/xkeysym |
| 467 | `xml-rs` | `0.8.29` | `MIT` | https://github.com/kornelski/xml-rs |
| 468 | `yoke` | `0.8.4` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 469 | `yoke-derive` | `0.8.4` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 470 | `zbus` | `4.4.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 471 | `zbus-lockstep` | `0.4.4` | `MIT` | https://github.com/luukvanderduim/zbus-lockstep |
| 472 | `zbus-lockstep-macros` | `0.4.4` | `MIT` | https://github.com/luukvanderduim/zbus-lockstep |
| 473 | `zbus_macros` | `4.4.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 474 | `zbus_names` | `3.0.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 475 | `zbus_xml` | `4.0.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 476 | `zerocopy` | `0.8.64` | `BSD-2-Clause OR Apache-2.0 OR MIT` | https://github.com/google/zerocopy |
| 477 | `zerocopy-derive` | `0.8.64` | `BSD-2-Clause OR Apache-2.0 OR MIT` | https://github.com/google/zerocopy |
| 478 | `zerofrom` | `0.1.8` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 479 | `zerofrom-derive` | `0.1.8` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 480 | `zeroize` | `1.9.2` | `Apache-2.0 OR MIT` | https://github.com/RustCrypto/utils |
| 481 | `zerotrie` | `0.2.5` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 482 | `zerovec` | `0.11.9` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 483 | `zerovec-derive` | `0.11.7` | `Unicode-3.0` | https://github.com/unicode-org/icu4x |
| 484 | `zlib-rs` | `0.6.8` | `Zlib` | https://github.com/trifectatechfoundation/zlib-rs |
| 485 | `zmij` | `1.0.23` | `MIT` | https://github.com/dtolnay/zmij |
| 486 | `zvariant` | `4.2.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 487 | `zvariant_derive` | `4.2.0` | `MIT` | https://github.com/dbus2/zbus/ |
| 488 | `zvariant_utils` | `2.1.0` | `MIT` | https://github.com/dbus2/zbus/ |

---

## 九、内嵌字体资源（非 Cargo 依赖）

- **Noto Sans SC Regular（子集）** — `assets/fonts/NotoSansSC-Regular-subset.ttf`
  - 版权：Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'.
  - 许可：**SIL Open Font License 1.1**（全文见 `assets/fonts/OFL-NotoSansSC.txt`）。
  - 用途：Android / Web 等没有系统 CJK 字体路径的环境内嵌渲染中文界面；桌面端仍优先使用系统字体。
  - 说明：该文件为项目内已有的 Noto Sans SC 子集（42 KB），非新增网络下载；子集可能不覆盖全部汉字，缺失字形由系统字体回退。
