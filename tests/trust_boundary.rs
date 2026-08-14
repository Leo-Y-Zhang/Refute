//! The one structural guard on the only security property this project has.
//!
//! `s VERIFIED` must be printed only when a checked sequence of steps derives
//! the empty clause. Discipline does not enforce that; a test that reads the
//! source does. It is crude, and it has caught this class of mistake before.

// A test asserts by panicking: `unwrap` on a fixture that must open, `panic!`
// on a verdict that must not happen, indexing a slice an assertion above it
// just sized. The package's panic floor in Cargo.toml is there for the library
// and the binary, where a panic on input-derived data is a denial of service.
// Here it would only make the failure report worse.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::{Path, PathBuf};

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn strip_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The files allowed to build the evidence that the empty clause was derived.
///
/// One per checker. Milestone 2 adds the second, and the point of naming them
/// here rather than counting them is that a *third* checker — or a helper that
/// grew a shortcut — has to be added to this list by hand, in a diff a reviewer
/// sees.
const WITNESS_SITES: [&str; 2] = ["checker.rs", "drat/checker.rs"];

/// Every occurrence of `needle` in the library, with the file it was found in.
///
/// The binary is excluded because it only ever *reads* a verdict — it cannot
/// construct one, since `Verdict` has no public constructor, no `Default` and
/// no `From`. Excluding it is what lets these counts be exact rather than
/// numbers that grow every time a caller is added.
fn library_sites(needles: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    rust_files(&source_root(), &mut files);
    files.retain(|p| !p.components().any(|c| c.as_os_str() == "bin"));
    assert!(!files.is_empty(), "found no library sources to scan");

    let mut sites = Vec::new();
    for path in &files {
        let code = strip_comments(&fs::read_to_string(path).unwrap());
        let count: usize = needles.iter().map(|n| code.matches(n).count()).sum();
        for _ in 0..count {
            sites.push(path.clone());
        }
    }
    sites
}

/// Exactly one site in the library constructs `Verdict::Verified`, and it is in
/// `verdict.rs`.
///
/// It used to be in `checker.rs`, and moved here when there was a second
/// checker. **Two checkers must not mean two doors.** Both now hand a witness
/// to `verdict::verified`, which is the only function that names the variant,
/// so the number this test watches stays one however many checkers the project
/// grows.
///
/// Both spellings count. Inside `verdict.rs`, `Self::Verified` builds exactly
/// what `Verdict::Verified` builds anywhere else — `impl Verdict { fn ok() ->
/// Self { Self::Verified } }` would be a second door, and a grep that knew
/// only the long form was watching the first one. Reading the variant by its
/// short name, in a `match` or a `matches!`, trips this as well: a false alarm
/// rather than a missed one, which is the right way round for this guard.
#[test]
fn verified_has_exactly_one_construction_site() {
    let sites = library_sites(&["Verdict::Verified", "Self::Verified"]);
    assert_eq!(
        sites.len(),
        1,
        "Verdict::Verified is constructed in {sites:?}; it must be constructed once, in verdict.rs"
    );
    assert!(
        sites[0].ends_with("verdict.rs"),
        "the single construction site moved to {:?}",
        sites[0]
    );
}

/// The evidence is built once per checker, in the checkers, and nowhere else.
///
/// `EmptyClauseDerived` carries nothing, so its whole value is that it cannot
/// be conjured: every construction is a literal `EmptyClauseDerived(())` that
/// this grep finds. A checker that reached `Verified` without building one
/// could not compile; a helper that built one on some other occasion would
/// show up here as a third site.
#[test]
fn the_empty_clause_witness_is_built_once_per_checker() {
    let sites = library_sites(&["EmptyClauseDerived(())"]);
    assert_eq!(
        sites.len(),
        WITNESS_SITES.len(),
        "the empty-clause witness is built in {sites:?}; expected {WITNESS_SITES:?}"
    );
    for expected in WITNESS_SITES {
        assert!(
            sites.iter().any(|p| p.ends_with(expected)),
            "no witness built in {expected}; sites are {sites:?}"
        );
    }
}

/// The variant is never imported, in the library or in the binary.
///
/// `use crate::verdict::Verdict::Verified;` — or a glob — makes a bare
/// `Verified` construct it, and every grep in this file would go on reporting
/// one construction site while any number of them existed. The import is the
/// hole; this is the test that closes it.
#[test]
fn the_verified_variant_is_never_imported() {
    let mut files = Vec::new();
    rust_files(&source_root(), &mut files);
    assert!(!files.is_empty(), "found no sources to scan");

    for path in &files {
        let code = strip_comments(&fs::read_to_string(path).unwrap());
        for line in code.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("use ") && !trimmed.starts_with("pub use ") {
                continue;
            }
            assert!(
                !line.contains("Verdict::Verified") && !line.contains("Verdict::*"),
                "{}: importing the variant hides every later construction of it: {}",
                path.display(),
                trimmed
            );
        }
    }
}

/// No route to a verdict that did not come from checking.
#[test]
fn verdict_has_no_default_and_no_conversion() {
    let source = fs::read_to_string(source_root().join("verdict.rs")).unwrap();
    let code = strip_comments(&source);
    assert!(
        !code.contains("impl Default for Verdict"),
        "Verdict must not have a Default"
    );
    assert!(
        !code.contains("for Verdict") || !code.contains("impl From"),
        "Verdict must not be constructible by conversion"
    );
    assert!(
        code.contains("#[must_use]"),
        "Verdict must be #[must_use]: a dropped verdict is a check that never happened"
    );
}

/// The CLI maps verdicts in one exhaustive match. A wildcard arm would let a
/// new variant fall silently into the success branch.
///
/// Anchored on the `Verdict::Verified` arm rather than on the name of the
/// function being matched, and the region runs to the end of the file because
/// the verdict match is the last thing `run` does. Code added after it that
/// used a wildcard would trip this test spuriously — a false alarm rather than
/// a missed one, which is the right way round for this particular guard.
#[test]
fn the_cli_match_on_verdict_has_no_wildcard_arm() {
    let source = fs::read_to_string(source_root().join("bin").join("refute.rs")).unwrap();
    let code = strip_comments(&source);
    let arm = code
        .find("Verdict::Verified =>")
        .expect("the CLI should match Verdict::Verified explicitly");
    let start = code
        .get(..arm)
        .unwrap()
        .rfind("match ")
        .expect("the Verified arm should sit inside a match");
    let region = code.get(start..).unwrap();

    for variant in [
        "Verdict::Verified",
        "Verdict::NotVerified",
        "Verdict::Unsupported",
    ] {
        assert!(
            region.contains(variant),
            "the verdict match must handle {variant} explicitly"
        );
    }
    assert!(
        !region.contains("_ =>"),
        "the verdict match must stay exhaustive"
    );
}
