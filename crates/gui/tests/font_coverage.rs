//! 内嵌字体覆盖守卫：i18n 文案与 GUI/核心硬编码字符串里出现的每个字符，
//! 都必须能在内嵌 Noto Sans SC 子集（或符号回退子集）的 cmap 中找到，
//! 否则 Android/Web 上会出现方块（tofu）。
//!
//! 该测试不依赖第三方 TTF 解析库：直接读 sfnt/cmap 表（format 4 / 12）。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CJK_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular-subset.ttf");
const SYMBOL_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoSansMath-Symbols-subset.ttf");
const ZH_TOML: &str = include_str!("../../../assets/i18n/zh-CN.toml");
const EN_TOML: &str = include_str!("../../../assets/i18n/en-US.toml");

fn be16(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}

fn be32(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn cmap_subtable(d: &[u8]) -> Option<(usize, u16)> {
    let tables = be16(d, 4) as usize;
    let mut cmap = None;
    for i in 0..tables {
        let rec = 12 + i * 16;
        if &d[rec..rec + 4] == b"cmap" {
            cmap = Some(be32(d, rec + 8) as usize);
        }
    }
    let cmap = cmap?;
    let count = be16(d, cmap + 2) as usize;
    let mut best: Option<(usize, u16, u8)> = None;
    for i in 0..count {
        let rec = cmap + 4 + i * 8;
        let pid = be16(d, rec);
        let eid = be16(d, rec + 2);
        let sub = cmap + be32(d, rec + 4) as usize;
        let format = be16(d, sub);
        let rank = match (pid, eid, format) {
            (3, 10, 12) => 0,
            (0, 4, 12) | (0, 6, 12) => 1,
            (3, 1, 4) => 2,
            (0, 3, 4) | (0, 0, 4) => 3,
            _ => continue,
        };
        if best.map(|b| rank < b.2).unwrap_or(true) {
            best = Some((sub, format, rank));
        }
    }
    best.map(|(sub, format, _)| (sub, format))
}

fn cmap_has(d: &[u8], c: u32) -> bool {
    let Some((sub, format)) = cmap_subtable(d) else {
        return false;
    };
    match format {
        4 if c <= 0xFFFF => {
            let c = c as u16;
            let seg = be16(d, sub + 6) as usize / 2;
            let end = sub + 14;
            let start = end + seg * 2 + 2;
            let delta = start + seg * 2;
            let range = delta + seg * 2;
            for i in 0..seg {
                let e = be16(d, end + i * 2);
                let s = be16(d, start + i * 2);
                if c < s || c > e {
                    continue;
                }
                let ro = be16(d, range + i * 2);
                if ro == 0 {
                    let gid = (c as i32 + be16(d, delta + i * 2) as i16 as i32) & 0xFFFF;
                    return gid != 0;
                }
                let addr = range + i * 2 + ro as usize + 2 * (c - s) as usize;
                if addr + 2 > d.len() {
                    return false;
                }
                let gid = be16(d, addr);
                if gid == 0 {
                    return false;
                }
                let gid = (gid as i32 + be16(d, delta + i * 2) as i16 as i32) & 0xFFFF;
                return gid != 0;
            }
            false
        }
        12 => {
            let groups = be32(d, sub + 12) as usize;
            for i in 0..groups {
                let g = sub + 16 + i * 12;
                if c >= be32(d, g) && c <= be32(d, g + 4) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// 收集 Rust 源码里字符串字面量中的字符（跳过注释与转义）。
fn collect_literals(src: &str, out: &mut BTreeSet<char>) {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                    if i + 1 < bytes.len() && bytes[i] == b'u' && bytes[i + 1] == b'{' {
                        while i < bytes.len() && bytes[i] != b'}' {
                            i += 1;
                        }
                    }
                    i += 1;
                    continue;
                }
                let ch = src[i..].chars().next().unwrap();
                out.insert(ch);
                i += ch.len_utf8();
            }
            i += 1;
        } else if bytes[i] == b'r'
            && i + 1 < bytes.len()
            && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#')
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while j < bytes.len() && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'"' {
                j += 1;
                let closing = format!("\"{}", "#".repeat(hashes));
                if let Some(end) = src[j..].find(&closing) {
                    for ch in src[j..j + end].chars() {
                        out.insert(ch);
                    }
                    i = j + end + closing.len();
                } else {
                    i = j;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

fn scan_dir(dir: &Path, out: &mut BTreeSet<char>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            scan_dir(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(text) = fs::read_to_string(&path) {
                collect_literals(&text, out);
            }
        }
    }
}

/// 允许没有字形的字符：控制符、零宽与变体选择符等格式字符。
fn ignorable(c: char) -> bool {
    matches!(c as u32, 0x00..=0x1F | 0x7F..=0x9F | 0x200B..=0x200F | 0xFE00..=0xFE0F)
}

fn required_chars() -> BTreeSet<char> {
    let mut required: BTreeSet<char> = BTreeSet::new();
    for text in [ZH_TOML, EN_TOML] {
        required.extend(text.chars());
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    scan_dir(&manifest.join("src"), &mut required);
    scan_dir(&manifest.join("../core/src"), &mut required);
    // 常见 UI 符号（ASCII 标点与数字由源码/i18n 覆盖）。
    for ch in "×÷±≤≥≈∞√∑°′″‰←↑→↓↔⚠●○◆■□▲▼▶◀★☆¥€£©®™§¶①②③④⑤⑥⑦⑧⑨⑩".chars()
    {
        required.insert(ch);
    }
    required.retain(|c| !ignorable(*c));
    required
}

#[test]
fn embedded_fonts_cover_all_ui_text() {
    let required = required_chars();
    assert!(
        required.len() > 700,
        "字符扫描疑似失效：只收集到 {} 个字符",
        required.len()
    );
    let cjk = required.iter().filter(|c| (**c as u32) >= 0x3400).count();
    assert!(cjk > 400, "扫描到的汉字过少：{cjk}");

    let missing: Vec<char> = required
        .iter()
        .copied()
        .filter(|c| {
            let cp = *c as u32;
            !cmap_has(CJK_FONT, cp) && !cmap_has(SYMBOL_FONT, cp)
        })
        .collect();

    assert!(
        missing.is_empty(),
        "内嵌字体缺少 {} 个界面字符：{}（请重新生成内嵌字体子集）",
        missing.len(),
        missing.iter().collect::<String>()
    );

    // 防止字体文件被替换成空壳。
    assert!(cmap_has(CJK_FONT, '的' as u32));
    assert!(cmap_has(SYMBOL_FONT, 0x21BB));
}
