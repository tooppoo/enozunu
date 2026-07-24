//! Reads and writes the machine-generated lock file.
//!
//! `enozunu.lock.json` freezes what each mutable Git selector resolved to, and is the only
//! machine-generated record `summon` reads back as a resolution input.
//! It records exactly the information the manifest cannot express: which commit a `branch` or
//! `tag` ref pointed at. Exact-revision, Gist, and local sources never appear here — their
//! reproducible identity (or the lack of one, for local sources) is already fixed by the manifest.
//! `.enozunu/provenance.json` stays a write-only execution record; see
//! docs/design/adr/20260724T021001Z_lockfile-based-reproducibility.md for the split.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::git::{CommitSha, GitSelector, ResolvedSource};

/// The lock file lives next to the manifest it locks, so a manifest override relocates both together.
pub const LOCK_FILE_NAME: &str = "enozunu.lock.json";
pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRecord {
    pub version: u32,
    pub entries: Vec<LockEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    pub url: String,
    pub selector: LockSelector,
    #[serde(with = "commit_sha_serde")]
    pub resolved_revision: CommitSha,
}

/// The mutable Git selector kinds, serialized in the same tagged `{type, value}` shape as provenance.
///
/// `Revision` is deliberately absent: an exact pin already lives in the manifest, and a lock entry
/// for it would create a second copy that can go stale against the declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum LockSelector {
    Branch(String),
    Tag(String),
}

impl LockSelector {
    fn to_git_selector(&self) -> GitSelector {
        match self {
            LockSelector::Branch(branch) => GitSelector::Branch(branch.clone()),
            LockSelector::Tag(tag) => GitSelector::Tag(tag.clone()),
        }
    }

    /// The serialized `type` tag, doubling as the sort key so file order matches the JSON vocabulary.
    fn kind(&self) -> &'static str {
        match self {
            LockSelector::Branch(_) => "branch",
            LockSelector::Tag(_) => "tag",
        }
    }

    fn value(&self) -> &str {
        match self {
            LockSelector::Branch(value) | LockSelector::Tag(value) => value,
        }
    }
}

/// Serializes `CommitSha` as its bare string and re-validates on read, so a `LockEntry` can only
/// ever hold a canonical revision and no separate validation pass is needed after deserialization.
mod commit_sha_serde {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::git::CommitSha;

    pub fn serialize<S: Serializer>(sha: &CommitSha, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(sha.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CommitSha, D::Error> {
        let raw = String::deserialize(deserializer)?;
        CommitSha::parse(&raw).ok_or_else(|| {
            D::Error::custom(format!(
                "`{raw}` is not a full lowercase SHA-1 commit id (40 hexadecimal characters)"
            ))
        })
    }
}

/// A `lock-parse` diagnostic with the recovery route appended.
///
/// A corrupt lock fails every mode including `--update`, so no flag can recover from this
/// diagnostic; the message itself must carry the way out.
fn lock_parse_diagnostic(path: &Path, detail: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::LockParse,
        format!(
            "{detail}; restore {} from version control, or delete it and run `enozunu summon` to regenerate it",
            path.display()
        ),
    )
}

/// Reads the lock file, returning `Ok(None)` when it does not exist.
///
/// The `version` field is checked on a raw JSON value before the record shape is deserialized,
/// so a future lock version with a different shape reports `unsupported-lock-version` instead of
/// a misleading `lock-parse`. Unknown fields are tolerated, keeping additive version-1 changes
/// readable by older builds.
pub fn read(path: &Path) -> Result<Option<LockRecord>, Diagnostic> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to read {}: {e}", path.display()),
            ));
        }
    };

    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        lock_parse_diagnostic(path, format!("failed to parse {}: {e}", path.display()))
    })?;
    let version = value.get("version").and_then(serde_json::Value::as_u64);
    match version {
        Some(version) if version == u64::from(LOCK_VERSION) => {}
        Some(version) => {
            return Err(Diagnostic::new(
                DiagnosticCode::UnsupportedLockVersion,
                format!(
                    "{} declares lock version {version}, but this build supports only version {LOCK_VERSION}",
                    path.display()
                ),
            ));
        }
        None => {
            return Err(lock_parse_diagnostic(
                path,
                format!(
                    "{} does not declare a numeric `version` field",
                    path.display()
                ),
            ));
        }
    }

    let record: LockRecord = serde_json::from_value(value).map_err(|e| {
        lock_parse_diagnostic(path, format!("failed to parse {}: {e}", path.display()))
    })?;
    Ok(Some(record))
}

/// How a lock write changed the file, reported so the CLI only announces real changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Created,
    Updated,
    Unchanged,
}

/// Writes the record, comparing serialized bytes first so an unchanged lock is never rewritten.
///
/// Skipping the no-op write keeps repeated `summon` runs from churning the file's mtime and keeps
/// the CLI's "updated" line tied to an actual content change.
///
/// The write lands in a same-directory temporary file that is renamed into place. A corrupt lock
/// fails every mode including `--update`, so a torn plain write — process kill, disk full — would
/// leave the project unable to summon at all; the atomic rename closes that window.
pub fn write(path: &Path, record: &LockRecord) -> Result<WriteOutcome, Diagnostic> {
    let json = serde_json::to_string_pretty(record).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to serialize lock record: {e}"),
        )
    })? + "\n";

    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to read {}: {e}", path.display()),
            ));
        }
    };
    if existing.as_deref() == Some(json.as_str()) {
        return Ok(WriteOutcome::Unchanged);
    }

    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, json).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", temp_path.display()),
        )
    })?;
    fs::rename(&temp_path, path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", path.display()),
        )
    })?;
    Ok(match existing {
        Some(_) => WriteOutcome::Updated,
        None => WriteOutcome::Created,
    })
}

/// Lock entries as a resolution-input map, keyed exactly like `resolve_git_sources` keys its requests.
pub fn locked_revisions(record: &LockRecord) -> HashMap<(String, GitSelector), CommitSha> {
    record
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.url.clone(), entry.selector.to_git_selector()),
                entry.resolved_revision.clone(),
            )
        })
        .collect()
}

/// Builds the lock record for this run from what actually resolved.
///
/// Rebuilding from the run's resolutions — rather than diffing against the previous record — is
/// what implements add, refresh, and prune with one code path: a source removed from the manifest
/// simply never resolves, so it never re-enters the lock.
/// Only mutable selectors produce entries; a `Revision` key is the manifest's own pin.
/// Returns an error if a resolver reports a non-canonical commit, which would otherwise poison the
/// lock file and fail every later read.
pub fn build(
    resolved: &HashMap<(String, GitSelector), ResolvedSource>,
) -> Result<LockRecord, Diagnostic> {
    let mut entries = Vec::new();
    for ((url, selector), source) in resolved {
        let lock_selector = match selector {
            GitSelector::Branch(branch) => LockSelector::Branch(branch.clone()),
            GitSelector::Tag(tag) => LockSelector::Tag(tag.clone()),
            GitSelector::Revision(_) => continue,
        };
        let resolved_revision = CommitSha::parse(&source.commit).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Io,
                format!(
                    "resolver reported non-canonical commit `{}` for `{url}`; refusing to record it in {LOCK_FILE_NAME}",
                    source.commit
                ),
            )
        })?;
        entries.push(LockEntry {
            url: url.clone(),
            selector: lock_selector,
            resolved_revision,
        });
    }
    // Deterministic order keeps the serialized file stable across runs, so version-control diffs
    // only ever show real resolution changes.
    entries.sort_by(|a, b| {
        (a.url.as_str(), a.selector.kind(), a.selector.value()).cmp(&(
            b.url.as_str(),
            b.selector.kind(),
            b.selector.value(),
        ))
    });
    Ok(LockRecord {
        version: LOCK_VERSION,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sha(hex_char: char) -> CommitSha {
        CommitSha::parse(&hex_char.to_string().repeat(40)).unwrap()
    }

    fn sample_record() -> LockRecord {
        LockRecord {
            version: LOCK_VERSION,
            entries: vec![
                LockEntry {
                    url: "https://example.com/repo".to_owned(),
                    selector: LockSelector::Branch("main".to_owned()),
                    resolved_revision: sha('a'),
                },
                LockEntry {
                    url: "https://example.com/repo".to_owned(),
                    selector: LockSelector::Tag("v1.0.0".to_owned()),
                    resolved_revision: sha('b'),
                },
            ],
        }
    }

    #[test]
    fn write_then_read_round_trips_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        let record = sample_record();

        assert_eq!(write(&path, &record).unwrap(), WriteOutcome::Created);
        assert_eq!(read(&path).unwrap(), Some(record));

        let written = fs::read_to_string(&path).unwrap();
        assert!(written.ends_with("\n"));
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["entries"][0]["selector"]["type"], "branch");
        assert_eq!(parsed["entries"][0]["selector"]["value"], "main");
        assert_eq!(parsed["entries"][0]["resolved_revision"], "a".repeat(40));
    }

    #[test]
    fn write_reports_whether_the_content_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        let mut record = sample_record();

        assert_eq!(write(&path, &record).unwrap(), WriteOutcome::Created);
        assert_eq!(write(&path, &record).unwrap(), WriteOutcome::Unchanged);

        record.entries[0].resolved_revision = sha('c');
        assert_eq!(write(&path, &record).unwrap(), WriteOutcome::Updated);
    }

    #[test]
    fn read_returns_none_for_a_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read(&tmp.path().join(LOCK_FILE_NAME)).unwrap(), None);
    }

    #[test]
    fn read_reports_invalid_json_as_lock_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        fs::write(&path, "{ not json").unwrap();

        let diag = read(&path).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::LockParse);
    }

    #[test]
    fn read_reports_a_missing_version_field_as_lock_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        fs::write(&path, "{\"entries\": []}\n").unwrap();

        let diag = read(&path).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::LockParse);
    }

    #[test]
    fn read_reports_a_future_version_as_unsupported_even_with_an_unknown_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        // A future version may change the entry shape entirely; the version check must win over
        // the shape mismatch so the report names the real problem.
        fs::write(&path, "{\"version\": 2, \"locked\": {}}\n").unwrap();

        let diag = read(&path).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsupportedLockVersion);
        assert!(diag.message.contains("version 2"));
    }

    #[test]
    fn read_reports_a_non_canonical_revision_as_lock_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        let cases = [
            "abc123",                                   // abbreviated
            "468AAC8CAED5F0C3B859B8286968E2C78E2B8760", // uppercase
        ];
        for revision in cases {
            fs::write(
                &path,
                format!(
                    "{{\"version\": 1, \"entries\": [{{\"url\": \"https://example.com/repo\", \"selector\": {{\"type\": \"branch\", \"value\": \"main\"}}, \"resolved_revision\": \"{revision}\"}}]}}\n"
                ),
            )
            .unwrap();

            let diag = read(&path).unwrap_err();
            assert_eq!(
                diag.code,
                DiagnosticCode::LockParse,
                "must reject `{revision}`"
            );
        }
    }

    #[test]
    fn read_tolerates_unknown_fields_within_version_1() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        fs::write(
            &path,
            "{\"version\": 1, \"entries\": [], \"generated_by\": \"future-enozunu\"}\n",
        )
        .unwrap();

        let record = read(&path).unwrap().unwrap();
        assert_eq!(record.entries, Vec::new());
    }

    #[test]
    fn write_reports_io_failure_when_the_target_is_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);
        fs::create_dir_all(&path).unwrap();

        let diag = write(&path, &sample_record()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    fn locked_revisions_keys_match_the_resolution_key_shape() {
        let map = locked_revisions(&sample_record());

        assert_eq!(
            map[&(
                "https://example.com/repo".to_owned(),
                GitSelector::Branch("main".to_owned())
            )],
            sha('a')
        );
        assert_eq!(
            map[&(
                "https://example.com/repo".to_owned(),
                GitSelector::Tag("v1.0.0".to_owned())
            )],
            sha('b')
        );
    }

    #[test]
    fn build_locks_only_mutable_selectors_and_sorts_entries() {
        let source = |commit: &CommitSha| ResolvedSource {
            commit: commit.as_str().to_owned(),
            content_root: PathBuf::from("/unused"),
        };
        let mut resolved = HashMap::new();
        resolved.insert(
            (
                "https://example.com/zebra".to_owned(),
                GitSelector::Branch("main".to_owned()),
            ),
            source(&sha('a')),
        );
        resolved.insert(
            (
                "https://example.com/repo".to_owned(),
                GitSelector::Tag("v1.0.0".to_owned()),
            ),
            source(&sha('b')),
        );
        resolved.insert(
            (
                "https://example.com/repo".to_owned(),
                GitSelector::Branch("main".to_owned()),
            ),
            source(&sha('c')),
        );
        resolved.insert(
            (
                "https://example.com/repo".to_owned(),
                GitSelector::Revision(sha('d')),
            ),
            source(&sha('d')),
        );

        let record = build(&resolved).unwrap();

        assert_eq!(record.version, LOCK_VERSION);
        let keys: Vec<(String, &'static str, String)> = record
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.url.clone(),
                    entry.selector.kind(),
                    entry.selector.value().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                (
                    "https://example.com/repo".to_owned(),
                    "branch",
                    "main".to_owned()
                ),
                (
                    "https://example.com/repo".to_owned(),
                    "tag",
                    "v1.0.0".to_owned()
                ),
                (
                    "https://example.com/zebra".to_owned(),
                    "branch",
                    "main".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn build_refuses_a_non_canonical_resolver_commit() {
        let mut resolved = HashMap::new();
        resolved.insert(
            (
                "https://example.com/repo".to_owned(),
                GitSelector::Branch("main".to_owned()),
            ),
            ResolvedSource {
                commit: "not-a-sha".to_owned(),
                content_root: PathBuf::from("/unused"),
            },
        );

        let diag = build(&resolved).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
        assert!(diag.message.contains("not-a-sha"));
    }
}
