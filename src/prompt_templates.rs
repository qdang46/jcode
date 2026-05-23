//! Prompt-template discovery for `jcode prompts list|show` (issue #4 MVP).
//!
//! A prompt template is a Markdown file that the user can later expand into
//! the editor via `/<name>`. The interactive expansion + autocomplete +
//! front-matter / arg substitution pieces are tracked separately; this module
//! is the discovery + display half so users can drop a template into the
//! documented directories and confirm jcode sees it before any UI work.
//!
//! Discovery order (project beats global on collision):
//!
//!   1. `<cwd>/.jcode/prompts/*.md` walking up the ancestor chain (closest
//!      to cwd wins for a given name).
//!   2. `~/.jcode/prompts/*.md` (user-global).
//!
//! Filenames without the `.md` extension become the command name. Names are
//! validated as kebab-case-friendly (ASCII alphanumeric + `-`/`_`); files
//! with other characters are reported as `invalid_name` and skipped.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One discovered prompt template.
#[derive(Debug, Clone)]
pub struct PromptTemplate {
    /// Command name (file stem). Used as `/<name>` for the future expansion
    /// flow.
    pub name: String,
    /// Source path on disk.
    pub path: PathBuf,
    /// Origin: `"project"` for `.jcode/prompts/` walked up from cwd,
    /// `"user"` for `~/.jcode/prompts/`.
    pub source: &'static str,
    /// Raw body (file contents). MVP keeps this opaque — front-matter and
    /// `{{name}}` placeholders are parsed in a follow-up PR.
    pub body: String,
}

pub fn is_valid_template_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn collect_dir_into(out: &mut BTreeMap<String, PromptTemplate>, dir: &Path, source: &'static str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        if ext.as_deref() != Some("md") {
            continue;
        }
        if !is_valid_template_name(stem) {
            continue;
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // Project path (closest-to-cwd) inserted first wins; user-global is
        // walked last in `discover` and uses `entry().or_insert(...)` so it
        // does not overwrite project entries.
        out.entry(stem.to_string())
            .or_insert_with(|| PromptTemplate {
                name: stem.to_string(),
                path: path.clone(),
                source,
                body,
            });
    }
}

/// Walk cwd ancestors then home for `.jcode/prompts/`. Closest-to-cwd wins.
pub fn discover() -> Vec<PromptTemplate> {
    discover_in(std::env::current_dir().ok().as_deref())
}

/// Same as `discover` but takes an explicit working dir for tests.
pub fn discover_in(working_dir: Option<&Path>) -> Vec<PromptTemplate> {
    let mut found: BTreeMap<String, PromptTemplate> = BTreeMap::new();

    // Walk project dirs cwd-first up to root so cwd-closest wins for any name.
    if let Some(start) = working_dir {
        let mut current: Option<&Path> = Some(start);
        while let Some(d) = current {
            let dir = d.join(".jcode").join("prompts");
            if dir.is_dir() {
                collect_dir_into(&mut found, &dir, "project");
            }
            current = d.parent();
        }
    }

    // Then user-global. or_insert keeps any project entry that won.
    if let Ok(home) = crate::storage::jcode_dir() {
        let global = home.join("prompts");
        if global.is_dir() {
            collect_dir_into(&mut found, &global, "user");
        }
    }

    found.into_values().collect()
}

/// Resolve a single template by name, preserving discovery precedence.
pub fn find_by_name(name: &str) -> Option<PromptTemplate> {
    discover().into_iter().find(|t| t.name == name)
}

/// Serializable summary for `jcode prompts list --json`.
#[derive(Debug, serde::Serialize)]
pub struct PromptTemplateSummary<'a> {
    pub name: &'a str,
    pub path: String,
    pub source: &'a str,
    pub bytes: usize,
}

impl<'a> From<&'a PromptTemplate> for PromptTemplateSummary<'a> {
    fn from(t: &'a PromptTemplate) -> Self {
        Self {
            name: &t.name,
            path: t.path.display().to_string(),
            source: t.source,
            bytes: t.body.len(),
        }
    }
}

pub fn run_list(json: bool) -> Result<()> {
    let templates = discover();
    if json {
        let summaries: Vec<PromptTemplateSummary> = templates.iter().map(Into::into).collect();
        println!("{}", serde_json::to_string_pretty(&summaries)?);
        return Ok(());
    }
    if templates.is_empty() {
        println!(
            "No prompt templates found. Drop Markdown files into `.jcode/prompts/` (project) or `~/.jcode/prompts/` (user)."
        );
        return Ok(());
    }
    println!("Discovered {} prompt template(s):", templates.len());
    for t in &templates {
        println!(
            "  /{:<24} [{}]  {}  ({} bytes)",
            t.name,
            t.source,
            t.path.display(),
            t.body.len()
        );
    }
    Ok(())
}

pub fn run_show(name: &str) -> Result<()> {
    let template =
        find_by_name(name).with_context(|| format!("prompt template '{name}' not found"))?;
    eprintln!(
        "# /{name}  [{}]  {}",
        template.source,
        template.path.display()
    );
    println!("{}", template.body.trim_end());
    Ok(())
}

/// Where `prompts new` should drop a freshly-scaffolded template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewLocation {
    /// `<cwd>/.jcode/prompts/<name>.md` (default; project-local).
    Project,
    /// `~/.jcode/prompts/<name>.md` (user-global).
    User,
}

/// Scaffold a new prompt-template file with a starter body.
///
/// Returns the absolute path the file was written to. Refuses to clobber an
/// existing file unless `force` is true.
pub fn run_new(name: &str, location: NewLocation, force: bool) -> Result<PathBuf> {
    if !is_valid_template_name(name) {
        anyhow::bail!("Template name '{name}' must be ASCII alphanumeric + '-' or '_'.");
    }

    let dir = match location {
        NewLocation::Project => {
            let cwd = std::env::current_dir().context("cannot resolve cwd")?;
            cwd.join(".jcode").join("prompts")
        }
        NewLocation::User => crate::storage::jcode_dir()
            .context("cannot resolve ~/.jcode")?
            .join("prompts"),
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let path = dir.join(format!("{name}.md"));
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists. Pass --force to overwrite.",
            path.display()
        );
    }

    let scaffold = format!(
        "---\n\
         description: TODO — what this prompt does\n\
         args:\n\
         - name: focus\n\
           required: false\n\
           default: bugs\n\
         ---\n\n\
         # {name}\n\n\
         Replace this body with the prompt jcode should expand when the user\n\
         types `/{name}` (or `/{name} <args>`).\n\n\
         Example placeholders supported by future expansion work:\n\
         - {{{{focus}}}} — bound to the `focus` arg above (default `bugs`).\n\n\
         Until expansion lands, the body is inserted verbatim into the editor.\n",
    );
    std::fs::write(&path, scaffold)
        .with_context(|| format!("failed to write {}", path.display()))?;

    println!("{}", path.display());
    Ok(path)
}

#[cfg(test)]
mod new_tests {
    use super::*;

    #[test]
    fn run_new_writes_starter_template_to_user_dir() {
        let _lock = crate::storage::lock_test_env();
        let prev = std::env::var_os("JCODE_HOME");
        let temp = tempfile::TempDir::new().expect("temp");
        crate::env::set_var("JCODE_HOME", temp.path());

        let path = run_new("review", NewLocation::User, false).expect("scaffold");
        assert_eq!(path, temp.path().join("prompts").join("review.md"));
        let body = std::fs::read_to_string(&path).expect("read back");
        assert!(body.starts_with("---\n"));
        assert!(body.contains("# review"));
        assert!(body.contains("`/review`"));

        // Refuses to clobber.
        let err = run_new("review", NewLocation::User, false).unwrap_err();
        assert!(err.to_string().contains("already exists"));

        // --force overrides.
        run_new("review", NewLocation::User, true).expect("force overwrite");

        if let Some(prev) = prev {
            crate::env::set_var("JCODE_HOME", prev);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }

    #[test]
    fn run_new_rejects_invalid_names() {
        let _lock = crate::storage::lock_test_env();
        let prev = std::env::var_os("JCODE_HOME");
        let temp = tempfile::TempDir::new().expect("temp");
        crate::env::set_var("JCODE_HOME", temp.path());

        for bad in ["bad name", "with$char", "", "../escape"] {
            let err = run_new(bad, NewLocation::User, false).unwrap_err();
            assert!(
                err.to_string().contains("must be ASCII alphanumeric"),
                "bad name {bad:?} not rejected: {err}"
            );
        }

        if let Some(prev) = prev {
            crate::env::set_var("JCODE_HOME", prev);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dir_overrides_user_global_on_name_collision() {
        let temp = tempfile::TempDir::new().expect("temp");
        let proj = temp.path().join(".jcode/prompts");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("review.md"), "PROJECT_REVIEW_BODY").unwrap();

        let _lock = crate::storage::lock_test_env();
        let prev = std::env::var_os("JCODE_HOME");
        let user_temp = tempfile::TempDir::new().expect("user");
        crate::env::set_var("JCODE_HOME", user_temp.path());
        let user_dir = user_temp.path().join("prompts");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(user_dir.join("review.md"), "USER_REVIEW_BODY").unwrap();

        let templates = discover_in(Some(temp.path()));

        if let Some(prev) = prev {
            crate::env::set_var("JCODE_HOME", prev);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }

        let review = templates
            .iter()
            .find(|t| t.name == "review")
            .expect("found");
        assert_eq!(review.body, "PROJECT_REVIEW_BODY");
        assert_eq!(review.source, "project");
    }

    #[test]
    fn invalid_names_are_skipped() {
        let temp = tempfile::TempDir::new().expect("temp");
        let proj = temp.path().join(".jcode/prompts");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("ok-name.md"), "ok").unwrap();
        std::fs::write(proj.join("bad name with spaces.md"), "bad").unwrap();
        std::fs::write(proj.join("with$char.md"), "bad").unwrap();
        std::fs::write(proj.join("not-markdown.txt"), "ignored").unwrap();

        let _lock = crate::storage::lock_test_env();
        let prev = std::env::var_os("JCODE_HOME");
        let user_temp = tempfile::TempDir::new().expect("user");
        crate::env::set_var("JCODE_HOME", user_temp.path());

        let templates = discover_in(Some(temp.path()));

        if let Some(prev) = prev {
            crate::env::set_var("JCODE_HOME", prev);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }

        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["ok-name"]);
    }

    #[test]
    fn ancestor_walk_finds_template_in_parent_jcode_dir() {
        let temp = tempfile::TempDir::new().expect("temp");
        let parent = temp.path();
        let proj = parent.join(".jcode/prompts");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("rev.md"), "BODY_FROM_PARENT").unwrap();
        let nested = parent.join("nested/deep");
        std::fs::create_dir_all(&nested).unwrap();

        let _lock = crate::storage::lock_test_env();
        let prev = std::env::var_os("JCODE_HOME");
        let user_temp = tempfile::TempDir::new().expect("user");
        crate::env::set_var("JCODE_HOME", user_temp.path());

        let templates = discover_in(Some(&nested));

        if let Some(prev) = prev {
            crate::env::set_var("JCODE_HOME", prev);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }

        let rev = templates.iter().find(|t| t.name == "rev").expect("found");
        assert_eq!(rev.body, "BODY_FROM_PARENT");
    }
}
