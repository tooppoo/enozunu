//! Selects a provider Skill for a consumer target in `enozunu.kdl` without hand editing.
//!
//! The command follows the shared manifest-edit discipline (see `manifest_edit`): the
//! selection decision uses the effective consumer value — direct declarations,
//! `use-same-skills` expansion, and everything else `manifest::parse` resolves — while the
//! edit itself only ever appends one direct `use-skills` declaration to the root manifest's
//! target block, creating that block when missing. `use-same-skills` references and their
//! referenced targets are never rewritten.

use std::path::Path;

use kdl::KdlDocument;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{self, TargetAi};
use crate::manifest_edit::{
    append_back, child_indent, child_mut, commit_edit, kdl_string, load_manifest_for_edit,
    parse_snippet_node,
};

/// What `use-skill` did to the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseSkillOutcome {
    /// A direct `use-skills` declaration was appended and the manifest replaced atomically.
    Selected,
    /// The target's effective selection already contains the Skill; nothing was written.
    AlreadySelected,
}

/// Selects `skill_id` for `consumer` in the manifest at `manifest_path`.
pub fn run_use_skill(
    manifest_path: &Path,
    consumer: &str,
    skill_id: &str,
) -> Result<UseSkillOutcome, Vec<Diagnostic>> {
    let target_ai = match consumer {
        "claude" => TargetAi::Claude,
        "codex" => TargetAi::Codex,
        other => {
            return Err(vec![Diagnostic::new(
                DiagnosticCode::UnsupportedConsumer,
                format!(
                    "`{other}` is not a supported consumer; supported consumers are `claude` and `codex`"
                ),
            )]);
        }
    };
    manifest::validate_name(skill_id, "skill").map_err(|d| vec![d])?;

    let (text, parsed) = load_manifest_for_edit(manifest_path)?;

    // The selection must reference an effectively declared Skill source; parse-time
    // validation enforces the same rule for hand-written selections.
    if !parsed.provider.skills.iter().any(|s| s.name == skill_id) {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::UnknownSourceReference,
            format!(
                "skill `{skill_id}` is not declared under `provider.skills`; add it with `enozunu add-skill` first"
            ),
        )]);
    }

    // The effective value decides the no-op: a Skill already selected directly, through
    // `use-same-skills`, or conditionally (a `when` block) is not selected again.
    let effective = match target_ai {
        TargetAi::Claude => &parsed.consumer.claude,
        TargetAi::Codex => &parsed.consumer.codex,
    };
    if effective
        .as_ref()
        .is_some_and(|t| t.use_skills.iter().any(|u| u.name == skill_id))
    {
        return Ok(UseSkillOutcome::AlreadySelected);
    }

    commit_edit(
        manifest_path,
        &text,
        &format!("selecting skill `{skill_id}` for `{consumer}`"),
        |doc| insert_use_skills(doc, target_ai, skill_id),
    )?;
    Ok(UseSkillOutcome::Selected)
}

/// Appends the `use-skills` declaration, creating the target block when missing.
///
/// The declaration lands at the end of the target block, which is the end of the target's
/// Skill selection sequence: selections concatenate in declaration order, so appending last
/// leaves every existing declaration — including `use-same-skills` — in place and unchanged.
fn insert_use_skills(doc: &mut KdlDocument, target_ai: TargetAi, skill_id: &str) {
    // `manifest::parse` succeeded, so the document has exactly one root node holding the
    // `consumer` block that validation requires.
    let root = &mut doc.nodes_mut()[0];
    let consumer =
        child_mut(root, "consumer").expect("a parsed manifest declares a `consumer` block");

    let declaration = format!("use-skills {}", kdl_string(skill_id));
    let Some(target) = child_mut(consumer, target_ai.as_str()) else {
        let indent = child_indent(consumer);
        let snippet = format!(
            "{target} {{\n{indent}  {declaration}\n{indent}}}",
            target = target_ai.as_str(),
        );
        append_back(consumer, parse_snippet_node(&snippet), &indent);
        return;
    };

    let indent = child_indent(target);
    append_back(target, parse_snippet_node(&declaration), &indent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_manifest(dir: &Path, text: &str) -> std::path::PathBuf {
        let path = dir.join("enozunu.kdl");
        fs::write(&path, text).unwrap();
        path
    }

    const SIMPLE_MANIFEST: &str = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n      skill \"deploy\" {\n        local {\n          path \"skills/deploy\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"deploy\"\n    }\n  }\n}\n";

    #[test]
    fn appends_a_direct_declaration_at_the_end_of_the_target_block() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), SIMPLE_MANIFEST);

        let outcome = run_use_skill(&manifest_path, "claude", "review").unwrap();

        assert_eq!(outcome, UseSkillOutcome::Selected);
        let written = fs::read_to_string(&manifest_path).unwrap();
        let expected = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n      skill \"deploy\" {\n        local {\n          path \"skills/deploy\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"deploy\"\n      use-skills \"review\"\n    }\n  }\n}\n";
        assert_eq!(written, expected);
    }

    #[test]
    fn creates_the_target_block_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(
            tmp.path(),
            "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"review\"\n    }\n  }\n}\n",
        );

        let outcome = run_use_skill(&manifest_path, "codex", "review").unwrap();

        assert_eq!(outcome, UseSkillOutcome::Selected);
        let written = fs::read_to_string(&manifest_path).unwrap();
        let expected = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"review\"\n    }\n    codex {\n      use-skills \"review\"\n    }\n  }\n}\n";
        assert_eq!(written, expected);
    }

    #[test]
    fn creates_the_target_block_in_an_empty_consumer() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(
            tmp.path(),
            "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n  }\n}\n",
        );

        run_use_skill(&manifest_path, "claude", "review").unwrap();

        let written = fs::read_to_string(&manifest_path).unwrap();
        assert!(
            written
                .contains("  consumer {\n    claude {\n      use-skills \"review\"\n    }\n  }\n"),
            "{written}"
        );
    }

    #[test]
    fn a_directly_selected_skill_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), SIMPLE_MANIFEST);

        let outcome = run_use_skill(&manifest_path, "claude", "deploy").unwrap();

        assert_eq!(outcome, UseSkillOutcome::AlreadySelected);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), SIMPLE_MANIFEST);
    }

    #[test]
    fn a_skill_selected_through_use_same_skills_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"review\"\n    }\n    codex {\n      use-same-skills \"claude\"\n    }\n  }\n}\n";
        let manifest_path = write_manifest(tmp.path(), manifest);

        let outcome = run_use_skill(&manifest_path, "codex", "review").unwrap();

        assert_eq!(outcome, UseSkillOutcome::AlreadySelected);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), manifest);
    }

    #[test]
    fn a_conditionally_selected_skill_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n    instructions {\n      claude {\n        local {\n          path \"docs/base.md\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-skills \"review\" {\n        when \"reviewing code\"\n      }\n    }\n  }\n}\n";
        let manifest_path = write_manifest(tmp.path(), manifest);

        let outcome = run_use_skill(&manifest_path, "claude", "review").unwrap();

        assert_eq!(outcome, UseSkillOutcome::AlreadySelected);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), manifest);
    }

    #[test]
    fn an_undeclared_skill_is_rejected_without_a_write() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), SIMPLE_MANIFEST);

        let diags = run_use_skill(&manifest_path, "claude", "missing").unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnknownSourceReference);
        assert!(diags[0].message.contains("enozunu add-skill"));
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), SIMPLE_MANIFEST);
    }

    #[test]
    fn an_unsupported_consumer_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), SIMPLE_MANIFEST);

        let diags = run_use_skill(&manifest_path, "cursor", "review").unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnsupportedConsumer);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), SIMPLE_MANIFEST);
    }

    #[test]
    fn an_invalid_skill_name_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), SIMPLE_MANIFEST);

        let diags = run_use_skill(&manifest_path, "claude", "bad/name").unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::InvalidName);
    }

    #[test]
    fn an_invalid_manifest_is_rejected_without_a_write() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = write_manifest(tmp.path(), "enozunu config-version=1 {\n}\n");

        let diags = run_use_skill(&manifest_path, "claude", "review").unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::ManifestShape);
        assert_eq!(
            fs::read_to_string(&manifest_path).unwrap(),
            "enozunu config-version=1 {\n}\n"
        );
    }

    #[test]
    fn unrelated_comments_and_declarations_are_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = "// Project manifest.\nenozunu config-version=1 {\n  provider {\n    agents {\n      agent \"helper\" {\n        local {\n          path \"agents/helper.md\"\n        }\n      }\n    }\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n      skill \"deploy\" {\n        local {\n          path \"skills/deploy\"\n        }\n      }\n    }\n  }\n  // A comment between blocks survives the edit.\n  consumer {\n    claude {\n      // Existing selections stay untouched.\n      use-skills \"deploy\"\n      use-agents \"helper\"\n    }\n  }\n}\n";
        let manifest_path = write_manifest(tmp.path(), manifest);

        run_use_skill(&manifest_path, "claude", "review").unwrap();

        let written = fs::read_to_string(&manifest_path).unwrap();
        assert!(written.contains("// A comment between blocks survives the edit."));
        assert!(written.contains("// Existing selections stay untouched."));
        assert!(written.contains("use-agents \"helper\"\n      use-skills \"review\"\n"));
    }
}
