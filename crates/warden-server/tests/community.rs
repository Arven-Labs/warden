//! The first source (P160), `community/`: the signed index and the packages kept in this repository. The CI has no signing key, so this
//! only checks that what is committed agrees with itself: the index is signed by `PUBLIC_KEY`, each entry points at a package file and
//! names its hash, each package is signed by the same key, and each package holds exactly the text of its `skills/*.md`. Edit a skill
//! and forget `community/build.sh`, and this fails.

use std::path::{Path, PathBuf};

use warden_core::skill::index::Index;
use warden_core::skill::package::Package;
use warden_core::skill::Skill;

const BASE_URL: &str = "https://raw.githubusercontent.com/Arven-Labs/warden/main/community/packages";

fn community() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../community")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn public_key() -> String {
    read(&community().join("PUBLIC_KEY")).trim().to_string()
}

fn index() -> Index {
    Index::parse_signed_by(&read(&community().join("index.json")), &public_key()).expect("index.json is signed by community/PUBLIC_KEY")
}

fn skill_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(community().join("skills"))
        .unwrap()
        .filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(str::to_string))
        .collect();
    names.sort();
    names
}

#[test]
fn the_public_key_is_a_real_key() {
    let key = public_key();
    assert_eq!(key.len(), 64, "{key}");
    assert!(key.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{key}");
}

#[test]
fn every_skill_has_an_entry_in_the_index_and_nothing_else_does() {
    let mut listed: Vec<String> = index().entries.iter().map(|e| e.id.clone()).collect();
    listed.sort();
    assert_eq!(listed, skill_names(), "the index lists the skills of community/skills, no more, no fewer");
}

#[test]
fn each_entry_points_at_its_package_file_and_names_its_hash() {
    for entry in &index().entries {
        assert_eq!(entry.source, format!("{BASE_URL}/{}.warden-skill.json", entry.id), "{}", entry.id);
        let package = Package::parse(&read(&community().join("packages").join(format!("{}.warden-skill.json", entry.id)))).unwrap_or_else(|e| panic!("{}: {e:#}", entry.id));
        assert_eq!(package.hash, entry.hash, "{}: the index names the hash of the package file as committed", entry.id);
        assert_eq!((package.id.as_str(), package.version.as_str(), package.license.as_str()), (entry.id.as_str(), entry.version.as_str(), entry.license.as_str()));
    }
}

#[test]
fn each_package_is_signed_by_the_key_and_holds_the_text_of_its_skill() {
    let prefix = format!("ed25519:{}:", public_key());
    for name in skill_names() {
        let package = Package::parse(&read(&community().join("packages").join(format!("{name}.warden-skill.json")))).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert!(package.signature.as_deref().is_some_and(|s| s.starts_with(&prefix)), "{name} is signed by community/PUBLIC_KEY");

        let skill = Skill::parse(&name, &read(&community().join("skills").join(format!("{name}.md"))));
        let packed = package.as_skill().expect("a skill package");
        assert_eq!(packed.name, skill.name, "{name}");
        assert_eq!(packed.description, skill.description.split_whitespace().collect::<Vec<_>>().join(" "), "{name}: the text changed, run community/build.sh");
        assert_eq!(packed.body, skill.body.trim(), "{name}: the text changed, run community/build.sh");
    }
}
