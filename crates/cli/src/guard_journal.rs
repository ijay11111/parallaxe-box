//! Privileged guard journal. Only records in the fixed machine directory authorize removal.
use fs2::FileExt;
use parallaxe_core::sysblock::{valid_rule_tag, Platform, SysBlockPlan};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Failure(pub &'static str, pub String);
impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self("SYSBLOCK_AUDIT_IO", e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Failure>;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Phase {
    Intent,
    Applied,
    RevokeIntent,
    Revoked,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    schema: String,
    status: Phase,
    pub rule_tag: String,
    pub run_id: String,
    pub platform: String,
    plan_id: String,
    created_at: String,
    pub candidates: BTreeMap<String, Vec<IpAddr>>,
    pub content_hash: String,
    pub marker: String,
    zero_residue: bool,
    zero_residue_scope: String,
}
impl Journal {
    fn digest(&self) -> String {
        let content = json!([
            "parallaxe.sysblock/outbound-block/v2",
            self.platform,
            self.rule_tag,
            self.candidates
        ]);
        format!("{:x}", Sha256::digest(content.to_string().as_bytes()))
    }
    fn marker(&self) -> String {
        format!("px:{}:{}:{}", self.rule_tag, self.run_id, self.content_hash)
    }
    fn new(plan: &SysBlockPlan, tag: &str) -> Result<Self> {
        let mut bytes = [0u8; 12];
        getrandom::fill(&mut bytes).map_err(|e| Failure("SYSBLOCK_RUN_ID", e.to_string()))?;
        let mut candidates = BTreeMap::<String, Vec<IpAddr>>::new();
        for entry in &plan.entries {
            candidates.entry(entry.fqdn.clone()).or_default().push(
                entry
                    .ip
                    .parse()
                    .map_err(|_| Failure("SYSBLOCK_INVALID_ADDRESS", entry.ip.clone()))?,
            );
        }
        let mut j = Self {
            schema: "parallaxe.sysblock.audit/v2".into(),
            status: Phase::Intent,
            rule_tag: tag.into(),
            run_id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            platform: plan.platform.clone(),
            plan_id: plan.source_plan_id.clone(),
            created_at: plan.created_at.clone(),
            candidates,
            content_hash: String::new(),
            marker: String::new(),
            zero_residue: false,
            zero_residue_scope: "KNOWN_RULES_THIS_RUN_ONLY".into(),
        };
        j.content_hash = j.digest();
        j.marker = j.marker();
        Ok(j)
    }
    pub fn ips(&self) -> Vec<IpAddr> {
        self.candidates
            .values()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn validate(&self, tag: &str, platform: Platform) -> Result<()> {
        if self.schema != "parallaxe.sysblock.audit/v2"
            || !valid_rule_tag(tag)
            || self.rule_tag != tag
            || self.platform != platform.as_str()
            || self.run_id.len() != 24
            || !self.run_id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.candidates.is_empty()
            || self.ips().is_empty()
            || self.content_hash != self.digest()
            || self.marker != self.marker()
        {
            return Err(Failure(
                "SYSBLOCK_AUDIT_INVALID",
                "无法验证本机意图记录。".into(),
            ));
        }
        Ok(())
    }
    fn document(&self) -> Value {
        let mut doc = json!(self);
        if self.status != Phase::Revoked {
            doc["recovery"] = json!(format!(
                "parallaxe guard unblock --rule-tag {}",
                self.rule_tag
            ));
            if self.status != Phase::Applied {
                doc["code"] = json!("SYSBLOCK_RECOVERY_REQUIRED");
                doc["detail"] = json!("上次系统写入状态未知，请先核对规则。");
            }
        }
        doc
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Seen {
    Absent,
    Owned(u64),
    Foreign,
}
pub trait Backend {
    fn inspect(&self, j: &Journal) -> Result<Seen>;
    fn create(&self, j: &Journal) -> Result<()>;
    fn remove(&self, j: &Journal, handle: u64) -> Result<()>;
}
pub fn directory() -> Result<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("ProgramFiles")
            .map(|p| PathBuf::from(p).join("Parallaxe/guard"))
            .ok_or_else(|| Failure("SYSBLOCK_AUDIT_IO", "ProgramFiles unavailable".into()))
    } else {
        Ok(PathBuf::from("/var/lib/parallaxe/guard"))
    }
}
fn journal_path(root: &Path, tag: &str) -> Result<PathBuf> {
    if !valid_rule_tag(tag) {
        return Err(Failure("SYSBLOCK_INVALID_TAG", "规则标签无效".into()));
    }
    Ok(root.join(format!("sysblock-{tag}.json")))
}
fn load(path: &Path, tag: &str, platform: Platform) -> Result<Option<Journal>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let j: Journal = serde_json::from_slice(&bytes)
        .map_err(|e| Failure("SYSBLOCK_AUDIT_INVALID", e.to_string()))?;
    j.validate(tag, platform)?;
    Ok(Some(j))
}
fn lock(path: &Path) -> Result<File> {
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    FileExt::try_lock_exclusive(&f).map_err(|e| Failure("SYSBLOCK_BUSY", e.to_string()))?;
    Ok(f)
}
fn save(path: &Path, j: &Journal, new_only: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(&serde_json::to_vec_pretty(&j.document()).expect("serializable journal"))?;
    tmp.as_file().sync_all()?;
    if new_only {
        tmp.persist_noclobber(path)
    } else {
        tmp.persist(path)
    }
    .map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn status(root: &Path, tag: &str, p: Platform) -> Result<Value> {
    let path = journal_path(root, tag)?;
    Ok(load(&path, tag, p)?
        .map(|j| j.document())
        .unwrap_or_else(|| json!({"status":"NO_AUDIT","rule_tag":tag})))
}
pub fn pending(root: &Path, tag: &str, p: Platform) -> Result<Option<Value>> {
    Ok(load(&journal_path(root, tag)?, tag, p)?
        .filter(|j| j.status != Phase::Revoked)
        .map(|j| j.document()))
}
fn foreign() -> Failure {
    Failure(
        "UNBLOCK_FOREIGN_RULE",
        "实时规则与本次审计不匹配，保留原规则。".into(),
    )
}
pub fn block(
    root: &Path,
    p: Platform,
    tag: &str,
    plan: &SysBlockPlan,
    extra: Option<&Path>,
    os: &dyn Backend,
) -> Result<Value> {
    let path = journal_path(root, tag)?;
    let _lock = lock(&path)?;
    if pending(root, tag, p)?.is_some() {
        return Err(Failure(
            "SYSBLOCK_RECOVERY_REQUIRED",
            format!("parallaxe guard unblock --rule-tag {tag}"),
        ));
    }
    let mut j = Journal::new(plan, tag)?;
    j.validate(tag, p)?;
    // Both durable intents must succeed before the first firewall mutation.
    save(&path, &j, false)?;
    if let Some(copy) = extra {
        save(copy, &j, true)?;
    }
    if os.inspect(&j)? != Seen::Absent {
        return Err(Failure("SYSBLOCK_RULE_EXISTS", "规则标签已占用".into()));
    }
    if let Err(create_error) = os.create(&j) {
        // A failed command may have committed. Never undo an unverified object.
        revoke_owned(&mut j, &path, os)?;
        if let Some(copy) = extra {
            save(copy, &j, false)?;
        }
        return Err(create_error);
    }
    if !matches!(os.inspect(&j), Ok(Seen::Owned(_))) {
        return Err(Failure(
            "SYSBLOCK_APPLY_UNVERIFIED",
            format!("写后回查未确认，请执行 parallaxe guard unblock --rule-tag {tag}"),
        ));
    }
    j.status = Phase::Applied;
    save(&path, &j, false)?;
    if let Some(copy) = extra {
        save(copy, &j, false)?;
    }
    Ok(j.document())
}
fn revoke_owned(j: &mut Journal, path: &Path, os: &dyn Backend) -> Result<()> {
    let read = |j: &Journal| {
        os.inspect(j)
            .map_err(|e| Failure("ROLLBACK_UNVERIFIED", e.1))
    };
    let before = read(j)?;
    if before == Seen::Foreign {
        return Err(foreign());
    }
    j.status = Phase::RevokeIntent;
    j.zero_residue = false;
    save(path, j, false)?;
    if let Seen::Owned(handle) = before {
        os.remove(j, handle)?;
    }
    match read(j)? {
        Seen::Absent => (),
        Seen::Foreign => return Err(foreign()),
        Seen::Owned(_) => return Err(Failure("ROLLBACK_UNVERIFIED", "规则仍存在".into())),
    }
    j.status = Phase::Revoked;
    j.zero_residue = true;
    save(path, j, false)
}
pub fn unblock(
    root: &Path,
    p: Platform,
    tag: &str,
    apply: bool,
    os: &dyn Backend,
) -> Result<Value> {
    let path = journal_path(root, tag)?;
    // Dry run without a journal is offline and side-effect-free.
    if load(&path, tag, p)?.is_none() {
        if apply {
            return Err(foreign());
        }
        return Ok(json!({"status":"DRY_RUN","code":"SYSBLOCK_NO_AUDIT","rule_tag":tag}));
    }
    let _lock = if apply { Some(lock(&path)?) } else { None };
    let mut j = load(&path, tag, p)?.ok_or_else(foreign)?;
    if !apply {
        let seen = os.inspect(&j)?;
        if seen == Seen::Foreign {
            return Err(foreign());
        }
        return Ok(
            json!({"status":"DRY_RUN","observed":if seen==Seen::Absent{"ABSENT"}else{"OWNED"},"audit":j.document(),"confirm":format!("parallaxe guard unblock --rule-tag {tag} --apply")}),
        );
    }
    revoke_owned(&mut j, &path, os)?;
    Ok(j.document())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parallaxe_core::sysblock::SysBlockEntry;
    use std::cell::{Cell, RefCell};
    struct Fake<'a> {
        path: &'a Path,
        live: RefCell<Option<String>>,
        writes: Cell<u8>,
        queries: Cell<u8>,
        visible: bool,
    }
    impl Backend for Fake<'_> {
        fn inspect(&self, j: &Journal) -> Result<Seen> {
            self.queries.set(self.queries.get() + 1);
            Ok(match self.live.borrow().as_deref() {
                None => Seen::Absent,
                Some(marker) if marker == j.marker => Seen::Owned(42),
                _ => Seen::Foreign,
            })
        }
        fn create(&self, j: &Journal) -> Result<()> {
            let saved = status(self.path, &j.rule_tag, Platform::Linux)?;
            eprintln!(
                "fake backend before create: persisted phase={}",
                saved["status"]
            );
            assert_eq!(saved["status"], "INTENT");
            assert_eq!(saved["marker"], j.marker);
            self.writes.set(self.writes.get() + 1);
            if self.visible {
                *self.live.borrow_mut() = Some(j.marker.clone());
            }
            Ok(())
        }
        fn remove(&self, j: &Journal, handle: u64) -> Result<()> {
            assert_eq!(handle, 42);
            assert_eq!(
                status(self.path, &j.rule_tag, Platform::Linux)?["status"],
                "REVOKE_INTENT"
            );
            self.writes.set(self.writes.get() + 1);
            self.live.borrow_mut().take();
            Ok(())
        }
    }
    fn os(path: &Path) -> Fake<'_> {
        Fake {
            path,
            live: Default::default(),
            writes: Cell::new(0),
            queries: Cell::new(0),
            visible: true,
        }
    }
    fn plan() -> SysBlockPlan {
        SysBlockPlan {
            schema: "preview".into(),
            rule_tag: "parallaxe-block-test".into(),
            platform: "linux".into(),
            engine: "NFTABLES".into(),
            created_at: "2026-11-23T00:00:00Z".into(),
            source_plan_id: "plan-1".into(),
            source_run_id: None,
            entries: vec![SysBlockEntry {
                fqdn: "example.org".into(),
                ip: "203.0.113.7".into(),
                family: "V4",
            }],
            warnings: vec![],
            requires_admin: true,
            apply_commands: vec![],
            rollback_commands: vec![],
            effect_scope: String::new(),
            zero_residue: false,
        }
    }
    #[test]
    fn audit_failure_prevents_all_system_writes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let os = os(root);
        let p = Platform::Linux;
        assert_eq!(
            block(
                root,
                p,
                "test",
                &plan(),
                Some(&root.join("missing/copy.json")),
                &os
            )
            .unwrap_err()
            .0,
            "SYSBLOCK_AUDIT_IO"
        );
        assert_eq!(os.writes.get(), 0);
        assert_eq!(os.queries.get(), 0);
        assert_eq!(
            status(root, "test", p).unwrap()["code"],
            "SYSBLOCK_RECOVERY_REQUIRED"
        );
        let bad = root.join("unusable");
        fs::write(&bad, b"file").unwrap();
        assert!(block(&bad, p, "test", &plan(), None, &os).is_err());
        assert_eq!(os.writes.get(), 0);
    }
    #[test]
    fn foreign_replacement_and_missing_or_corrupt_journal_never_delete() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let os = os(root);
        let p = Platform::Linux;
        assert_eq!(
            unblock(root, p, "test", true, &os).unwrap_err().0,
            "UNBLOCK_FOREIGN_RULE"
        );
        let applied = block(root, p, "test", &plan(), None, &os).unwrap();
        assert_eq!(applied["status"], "APPLIED");
        assert_eq!(applied["zero_residue"], false);
        *os.live.borrow_mut() = Some("different-run".into());
        for apply in [false, true] {
            let code = unblock(root, p, "test", apply, &os).unwrap_err().0;
            eprintln!(
                "foreign replacement: apply={apply} code={code} writes={}",
                os.writes.get()
            );
            assert_eq!(code, "UNBLOCK_FOREIGN_RULE");
        }
        let mut corrupt = applied;
        corrupt["candidates"]["example.org"] = json!(["203.0.113.8"]);
        fs::write(journal_path(root, "test").unwrap(), corrupt.to_string()).unwrap();
        assert_eq!(
            unblock(root, p, "test", true, &os).unwrap_err().0,
            "SYSBLOCK_AUDIT_INVALID"
        );
        assert_eq!(os.writes.get(), 1);
    }
    #[test]
    fn unconfirmed_write_survives_restart_and_recovers_with_or_without_rule() {
        for present in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let mut os = os(root);
            os.visible = false;
            let p = Platform::Linux;
            assert_eq!(
                block(root, p, "test", &plan(), None, &os).unwrap_err().0,
                "SYSBLOCK_APPLY_UNVERIFIED"
            );
            let prior = pending(root, "test", p).unwrap().unwrap();
            assert_eq!(prior["recovery"], "parallaxe guard unblock --rule-tag test");
            eprintln!("restart phase={} code={}", prior["status"], prior["code"]);
            assert_eq!(prior["status"], "INTENT");
            assert_eq!(
                block(root, p, "test", &plan(), None, &os).unwrap_err().0,
                "SYSBLOCK_RECOVERY_REQUIRED"
            );
            if present {
                *os.live.borrow_mut() = Some(prior["marker"].as_str().unwrap().into());
            }
            assert_eq!(
                unblock(root, p, "test", false, &os).unwrap()["observed"],
                if present { "OWNED" } else { "ABSENT" }
            );
            let end = unblock(root, p, "test", true, &os).unwrap();
            eprintln!(
                "owned_rule_present={present} phase={} scope={} fake_writes={}",
                end["status"],
                end["zero_residue_scope"],
                os.writes.get()
            );
            assert_eq!(end["status"], "REVOKED");
            assert_eq!(end["zero_residue_scope"], "KNOWN_RULES_THIS_RUN_ONLY");
            assert_eq!(end["zero_residue"], true);
            assert_eq!(os.writes.get(), 1 + u8::from(present));
            assert!(pending(root, "test", p).unwrap().is_none());
        }
    }
}
