use crate::guard_journal::{Backend, Failure, Journal, Result, Seen};
use parallaxe_core::sysblock::Platform;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::Write,
    net::IpAddr,
    process::{Command, Stdio},
};
pub struct System(pub Platform);
fn run(program: &str, args: &[&str], input: &str) -> Result<String> {
    let failure = |e: std::io::Error| Failure("SYSBLOCK_OS_ERROR", e.to_string());
    let executable = match program {
        "powershell.exe" => std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .map(|p| p.join("System32/WindowsPowerShell/v1.0/powershell.exe")),
        "nft" => ["/usr/sbin/nft", "/sbin/nft", "/usr/bin/nft"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_file()),
        _ => None,
    }
    .ok_or_else(|| Failure("SYSBLOCK_OS_ERROR", "系统防火墙工具不可用".into()))?;
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failure)?;
    let input_result = child.stdin.take().unwrap().write_all(input.as_bytes());
    let output = child.wait_with_output().map_err(failure)?;
    input_result.map_err(failure)?;
    if !output.status.success() {
        return Err(Failure(
            "SYSBLOCK_OS_ERROR",
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| Failure("SYSBLOCK_OS_ERROR", e.to_string()))
}
fn windows(j: &Journal, op: &str) -> Result<String> {
    run(
        "powershell.exe",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("guard_windows.ps1"),
        ],
        &json!({"op":op,"tag":j.rule_tag,"run":j.run_id,"marker":j.marker,"ips":j.ips()})
            .to_string(),
    )
}
fn table(j: &Journal) -> String {
    format!("parallaxe_{}", j.rule_tag)
}
fn ruleset(j: &Journal) -> String {
    let t = table(j);
    let marker = &j.marker;
    let mut text=format!("create table inet {t} {{ comment \"{marker}\"; }}\ncreate chain inet {t} output {{ type filter hook output priority 0; policy accept; comment \"{marker}\"; }}\n");
    for (i, ip) in j.ips().iter().enumerate() {
        let family = if ip.is_ipv4() { "ip" } else { "ip6" };
        text.push_str(&format!(
            "add rule inet {t} output {family} daddr {ip} drop comment \"{}\"\n",
            rule_marker(j, i)
        ));
    }
    text
}
fn rule_marker(j: &Journal, i: usize) -> String {
    // 125 ASCII bytes at the longest tag, inside nft's comment limit.
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("{}:{i}", j.content_hash));
    format!("px:{}:{}:{hash:x}", j.rule_tag, j.run_id)
}
fn nft_json(args: &[&str]) -> Result<Vec<Value>> {
    let doc: Value = serde_json::from_str(&run("nft", args, "")?)
        .map_err(|e| Failure("SYSBLOCK_READBACK_INVALID", e.to_string()))?;
    doc["nftables"]
        .as_array()
        .cloned()
        .ok_or_else(|| Failure("SYSBLOCK_READBACK_INVALID", "missing nftables array".into()))
}
fn linux_snapshot(j: &Journal, objects: &[Value]) -> Seen {
    let t = table(j);
    let mut handle = None;
    let mut chain = false;
    let mut found = BTreeSet::new();
    let ips = j.ips();
    for object in objects {
        if object.get("metainfo").is_some() {
            continue;
        }
        if let Some(v) = object.get("table") {
            if handle.is_some()
                || v["name"] != t
                || v["family"] != "inet"
                || v["comment"] != j.marker
            {
                return Seen::Foreign;
            }
            handle = v["handle"].as_u64();
        } else if let Some(v) = object.get("chain") {
            if chain
                || v["family"] != "inet"
                || v["table"] != t
                || v["name"] != "output"
                || v["type"] != "filter"
                || v["hook"] != "output"
                || v["prio"] != 0
                || v["policy"] != "accept"
                || v["comment"] != j.marker
            {
                return Seen::Foreign;
            }
            chain = true;
        } else if let Some(v) = object.get("rule") {
            if v["table"] != t || v["family"] != "inet" || v["chain"] != "output" {
                return Seen::Foreign;
            }
            let expr = &v["expr"];
            let m = &expr[0]["match"];
            let Some(ip) = m["right"].as_str().and_then(|s| s.parse::<IpAddr>().ok()) else {
                return Seen::Foreign;
            };
            let Ok(i) = ips.binary_search(&ip) else {
                return Seen::Foreign;
            };
            if v["comment"] != rule_marker(j, i)
                || *expr
                    != json!([{"match":{"op":"==","left":{"payload":{"protocol":if ip.is_ipv4(){"ip"}else{"ip6"},"field":"daddr"}},"right":ip}},{"drop":null}])
                || !found.insert(ip)
            {
                return Seen::Foreign;
            }
        } else {
            return Seen::Foreign;
        }
    }
    match handle {
        Some(h) if chain && found.len() == ips.len() => Seen::Owned(h),
        _ => Seen::Foreign,
    }
}
impl Backend for System {
    fn inspect(&self, j: &Journal) -> Result<Seen> {
        match self.0 {
            Platform::Windows => match windows(j, "inspect")?.trim() {
                "ABSENT" => Ok(Seen::Absent),
                "OWNED" => Ok(Seen::Owned(0)),
                "FOREIGN" => Ok(Seen::Foreign),
                _ => Err(Failure(
                    "SYSBLOCK_READBACK_INVALID",
                    "unexpected rule query response".into(),
                )),
            },
            Platform::Linux => {
                let t = table(j);
                let tables = nft_json(&["-j", "list", "tables"])?;
                if !tables
                    .iter()
                    .any(|o| o["table"]["name"] == t && o["table"]["family"] == "inet")
                {
                    return Ok(Seen::Absent);
                }
                Ok(linux_snapshot(
                    j,
                    &nft_json(&["-j", "-a", "-n", "list", "table", "inet", &t])?,
                ))
            }
        }
    }
    fn create(&self, j: &Journal) -> Result<()> {
        match self.0 {
            Platform::Windows => windows(j, "create")?,
            Platform::Linux => run("nft", &["-f", "-"], &ruleset(j))?,
        };
        Ok(())
    }
    fn remove(&self, j: &Journal, handle: u64) -> Result<()> {
        let foreign = || Failure("UNBLOCK_FOREIGN_RULE", "规则归属已改变".into());
        match self.0 {
            Platform::Windows => {
                if windows(j, "remove")?.trim() != "OWNED" {
                    return Err(foreign());
                }
            }
            Platform::Linux => {
                #[cfg(target_os = "linux")]
                crate::nft_generation::remove_if_current(handle, || {
                    if self.inspect(j)? == Seen::Owned(handle) {
                        Ok(())
                    } else {
                        Err(foreign())
                    }
                })?;
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = handle;
                    return Err(Failure("SYSBLOCK_UNSUPPORTED", "Linux only".into()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nft_readback_requires_exact_table_chain_and_complete_owned_rules() {
        let journal:Journal=serde_json::from_value(json!({"schema":"parallaxe.sysblock.audit/v2","status":"INTENT","rule_tag":"sample","run_id":"0123456789abcdef01234567","platform":"linux","plan_id":"p","created_at":"t","candidates":{"example.org":["203.0.113.7"]},"content_hash":"00".repeat(32),"marker":"test-marker","zero_residue":false,"zero_residue_scope":"KNOWN_RULES_THIS_RUN_ONLY"})).unwrap();
        let base = json!({"table":"parallaxe_sample","family":"inet"});
        let mut chain = base.clone();
        let mut rule = base;
        for (k,v) in json!({"name":"output","type":"filter","hook":"output","prio":0,"policy":"accept","comment":journal.marker}).as_object().unwrap(){chain[k]=v.clone();}
        rule["chain"] = json!("output");
        rule["comment"] = json!(rule_marker(&journal, 0));
        rule["expr"] = json!([{"match":{"op":"==","left":{"payload":{"protocol":"ip","field":"daddr"}},"right":"203.0.113.7"}},{"drop":null}]);
        let owned = vec![
            json!({"table":{"name":"parallaxe_sample","family":"inet","handle":42,"comment":journal.marker}}),
            json!({"chain":chain}),
            json!({"rule":rule}),
        ];
        assert_eq!(linux_snapshot(&journal, &owned), Seen::Owned(42));
        assert_eq!(linux_snapshot(&journal, &owned[..2]), Seen::Foreign);
        for (pointer, value) in [
            ("/rule/comment", json!("other")),
            ("/rule/expr/0/match/right", json!("203.0.113.8")),
        ] {
            let mut changed = owned.clone();
            *changed[2].pointer_mut(pointer).unwrap() = value;
            assert_eq!(linux_snapshot(&journal, &changed), Seen::Foreign);
        }
        let mut foreign = owned;
        foreign.push(json!({"set":{"name":"external"}}));
        assert_eq!(linux_snapshot(&journal, &foreign), Seen::Foreign);
    }
}
