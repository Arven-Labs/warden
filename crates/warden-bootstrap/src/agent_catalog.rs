//! The gallery of ready-made agents (P140): a handful of agents that ship inside the binary, one file each in
//! `catalog/agents/<id>.md` (frontmatter plus the persona as the body), that a person adds with one click. Nothing is
//! downloaded and nothing is signed, because the text is part of the program they already trust; a catalog that comes from
//! outside (the signed `.wpkg` packages of `HOSPEDAGEM.md` section E) is a later step.
//!
//! A ready agent is born careful, the same shape as one another agent creates (`org_edit`, `AddReport`): the safe tools only,
//! every permission off, it asks before every change and has no model of its own (it uses the one the conversation has). A person
//! raises that later. It names the skills of the skills gallery it works with; installing the agent puts those in the vault too
//! (`install_skills`), without replacing a skill the person already has under that name.
//!
//! There is no record of what was installed. An entry's state comes from comparing it with the agent of that id in the config.

use std::sync::OnceLock;

use warden_core::autonomy::Category;
use warden_core::skill::catalog as skill_catalog;
use warden_core::skill::SkillStore;

use crate::manage_agents::ASK_FIRST_LEVEL;
use crate::{AgentConfig, FileConfig, SAFE_AGENT_TOOLS};

/// The files of `catalog/agents/`, named after the agent they hold.
const FILES: &[(&str, &str)] = &[
    ("code-reviewer", include_str!("../catalog/agents/code-reviewer.md")),
    ("researcher", include_str!("../catalog/agents/researcher.md")),
    ("secretary", include_str!("../catalog/agents/secretary.md")),
    ("writer", include_str!("../catalog/agents/writer.md")),
];

/// One ready-made agent: what the gallery shows and what installing writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogAgent {
    pub id: String,
    pub role: String,
    /// One line for the gallery. It is not stored in the config (an agent has no description).
    pub description: String,
    /// The skills of the skills gallery this agent works with.
    pub skills: Vec<String>,
    /// The persona: the system prompt, sent verbatim.
    pub persona: String,
}

impl CatalogAgent {
    /// Reads a file of `catalog/agents/`. Lines of the frontmatter that are not known are ignored.
    fn parse(id: &str, raw: &str) -> CatalogAgent {
        let raw = raw.replace("\r\n", "\n");
        let (mut role, mut description, mut skills) = (String::new(), String::new(), Vec::new());
        let mut body = raw.as_str();
        if let Some(rest) = raw.strip_prefix("---\n") {
            if let Some(end) = rest.find("\n---") {
                let (front, after) = rest.split_at(end);
                for line in front.lines() {
                    if let Some((key, value)) = line.split_once(':') {
                        match key.trim() {
                            "role" => role = value.trim().to_string(),
                            "description" => description = value.trim().to_string(),
                            "skills" => skills = value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
                            _ => {}
                        }
                    }
                }
                body = after["\n---".len()..].trim_start_matches('\n');
            }
        }
        CatalogAgent { id: id.to_string(), role, description, skills, persona: body.trim().to_string() }
    }

    /// The agent as it is written to the config: careful, with the safe tools and no model of its own.
    fn to_config(&self) -> AgentConfig {
        AgentConfig {
            id: self.id.clone(),
            persona: self.persona.clone(),
            provider_id: None,
            // Only a person turns these on, in Settings.
            can_delegate_to_agents: false,
            can_manage_agents: false,
            can_message_agents: false,
            can_manage_tasks: false,
            allowed_tools: Some(SAFE_AGENT_TOOLS.iter().map(|t| t.to_string()).collect()),
            autonomy: ASK_FIRST_LEVEL,
            approval_required: Category::ALL.to_vec(),
            role: Some(self.role.clone()),
            reports_to: None,
            owner: None,
            shared_with: Vec::new(),
            delegation_models: Vec::new(),
            can_start_tasks: false,
            can_create_workers: false,
            can_message_user: false,
            can_choose_models: false,
        }
    }
}

/// Where an entry of the gallery stands in a config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCatalogStatus {
    /// No agent of that id.
    NotInstalled,
    /// The config has this agent, with the same persona, role and tools (what the person changed in its powers does not count).
    Installed,
    /// The config has an agent of that id that differs: installing again replaces its text. A member's agent counts here too, and
    /// is never replaced.
    Modified,
}

impl AgentCatalogStatus {
    /// The word the wire uses for it.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentCatalogStatus::NotInstalled => "notInstalled",
            AgentCatalogStatus::Installed => "installed",
            AgentCatalogStatus::Modified => "modified",
        }
    }
}

/// Every agent of the gallery, sorted by id. Parsed once.
pub fn entries() -> &'static [CatalogAgent] {
    static ENTRIES: OnceLock<Vec<CatalogAgent>> = OnceLock::new();
    ENTRIES.get_or_init(|| FILES.iter().map(|(id, raw)| CatalogAgent::parse(id, raw)).collect())
}

fn same_text(found: &AgentConfig, entry: &CatalogAgent) -> bool {
    let safe: Vec<String> = SAFE_AGENT_TOOLS.iter().map(|t| t.to_string()).collect();
    found.owner.is_none()
        && found.persona.trim() == entry.persona
        && found.role.as_deref().map(str::trim) == Some(entry.role.as_str())
        && found.allowed_tools.as_deref() == Some(safe.as_slice())
}

/// The state of `entry` in `config`.
pub fn status(config: &FileConfig, entry: &CatalogAgent) -> AgentCatalogStatus {
    match config.agents.iter().find(|a| a.id == entry.id) {
        None => AgentCatalogStatus::NotInstalled,
        Some(found) if same_text(found, entry) => AgentCatalogStatus::Installed,
        Some(_) => AgentCatalogStatus::Modified,
    }
}

/// Puts the agent called `id` in `config`. An agent of that id that differs is only replaced with `overwrite`, so a click on
/// "Install" never throws away what a person wrote, and what is replaced is only the persona, the role and the tools: the powers,
/// the superior, the sharing and the model the person gave it stay. One that is already identical is left alone, and a member's
/// agent is never touched. The caller still runs the checks of a settings save (`check_agents`) before writing.
pub fn install(config: &mut FileConfig, id: &str, overwrite: bool) -> Result<(), String> {
    let entry = entries().iter().find(|e| e.id == id).ok_or_else(|| format!("the gallery has no agent named '{id}'"))?;
    match config.agents.iter_mut().find(|a| a.id == entry.id) {
        None => config.agents.push(entry.to_config()),
        Some(found) if found.owner.is_some() => return Err(format!("'{id}' is an agent of a member of your workspace; pick another name for it first")),
        Some(found) if same_text(found, entry) => {}
        Some(_) if !overwrite => {
            return Err(format!("an agent named '{id}' is already in your agents and differs from this one; installing replaces its text"));
        }
        Some(found) => {
            let fresh = entry.to_config();
            found.persona = fresh.persona;
            found.role = fresh.role;
            found.allowed_tools = fresh.allowed_tools;
        }
    }
    Ok(())
}

/// What `install_skills` did with the skills an agent works with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillsInstalled {
    /// Put in the vault just now.
    pub installed: Vec<String>,
    /// Left as they were, because the person has a skill with that name that differs from the gallery's.
    pub kept: Vec<String>,
}

/// The skills `agent` works with that the person already has in a version that differs from the gallery's: installing the agent
/// leaves them as they are. The gallery shows this before the person installs, and after.
pub fn skills_kept(store: &SkillStore, agent: &CatalogAgent) -> Vec<String> {
    agent
        .skills
        .iter()
        .filter(|name| {
            skill_catalog::entries()
                .iter()
                .find(|e| &e.name == *name)
                .is_some_and(|entry| skill_catalog::status(store, entry) == skill_catalog::CatalogStatus::Modified)
        })
        .cloned()
        .collect()
}

/// Puts the skills `agent` works with in the vault, the way the skills gallery does and without replacing any: one already there
/// and identical is fine, one that differs stays as the person made it and is listed in `kept`.
pub fn install_skills(store: &SkillStore, agent: &CatalogAgent) -> SkillsInstalled {
    let mut done = SkillsInstalled::default();
    for name in &agent.skills {
        let Some(entry) = skill_catalog::entries().iter().find(|e| &e.name == name) else { continue };
        match skill_catalog::status(store, entry) {
            skill_catalog::CatalogStatus::Installed => {}
            skill_catalog::CatalogStatus::Modified => done.kept.push(name.clone()),
            skill_catalog::CatalogStatus::NotInstalled => {
                if skill_catalog::install(store, name, false).is_ok() {
                    done.installed.push(name.clone());
                }
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use warden_core::memory::Vault;

    use super::*;
    use crate::manage_agents::{check_id, check_persona, check_role};

    fn temp_store() -> SkillStore {
        let dir = std::env::temp_dir().join(format!(
            "warden-agent-catalog-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        SkillStore::new(Arc::new(Vault::new(dir)))
    }

    #[test]
    fn every_entry_is_a_valid_careful_agent_with_a_unique_id_that_matches_its_file() {
        assert!(!entries().is_empty());
        let mut seen = Vec::new();
        for ((file_id, _), entry) in FILES.iter().zip(entries()) {
            assert_eq!(&entry.id, file_id);
            assert!(!seen.contains(&entry.id), "'{}' is in the gallery twice", entry.id);
            seen.push(entry.id.clone());
            check_id(&entry.id).unwrap_or_else(|e| panic!("'{}': {e:#}", entry.id));
            check_persona(&entry.persona).unwrap_or_else(|e| panic!("'{}': {e:#}", entry.id));
            assert!(check_role(Some(&entry.role)).unwrap().is_some(), "'{}' has no role", entry.id);
            assert!(!entry.description.is_empty(), "'{}' has no description", entry.id);
            assert!(entry.description.chars().count() <= 200, "'{}' has a long description", entry.id);

            // Careful by default: the safe tools only, every permission off, it asks before every change, and no model of its own.
            let agent = entry.to_config();
            assert_eq!(agent.allowed_tools.as_deref().unwrap().len(), SAFE_AGENT_TOOLS.len());
            assert_eq!(agent.autonomy, ASK_FIRST_LEVEL);
            assert_eq!(agent.approval_required, Category::ALL.to_vec());
            assert!(agent.provider_id.is_none() && agent.owner.is_none() && agent.reports_to.is_none());
            assert!(!agent.can_delegate_to_agents && !agent.can_manage_agents && !agent.can_message_agents && !agent.can_manage_tasks);
            assert!(!agent.can_start_tasks && !agent.can_create_workers && !agent.can_message_user && !agent.can_choose_models);
        }
    }

    #[test]
    fn every_skill_an_agent_names_is_in_the_skills_gallery_and_the_agent_can_use_skills() {
        for entry in entries() {
            assert!(!entry.skills.is_empty(), "'{}' works with no skill", entry.id);
            for skill in &entry.skills {
                assert!(skill_catalog::entries().iter().any(|e| &e.name == skill), "'{}' names '{skill}', which the skills gallery does not have", entry.id);
                assert!(entry.persona.contains(&format!("`{skill}`")) || entry.persona.contains("use_skill"), "'{}' never tells the model about '{skill}'", entry.id);
            }
            let tools = entry.to_config().allowed_tools.unwrap();
            assert!(tools.contains(&"use_skill".to_string()) && tools.contains(&"read_skill_file".to_string()), "'{}' cannot load a skill", entry.id);
        }
    }

    #[test]
    fn the_gallery_is_only_text_with_no_links_and_no_commands() {
        for entry in entries() {
            let text = format!("{}\n{}\n{}", entry.role, entry.description, entry.persona).to_lowercase();
            for forbidden in ["http://", "https://", "www.", "```", "curl ", "sudo ", "rm -rf", "| sh", "api_key", "password:"] {
                assert!(!text.contains(forbidden), "'{}' contains '{forbidden}'", entry.id);
            }
        }
    }

    #[test]
    fn an_entry_goes_from_not_installed_to_installed_to_modified() {
        let mut config = FileConfig::default();
        let entry = &entries()[0];
        assert_eq!(status(&config, entry), AgentCatalogStatus::NotInstalled);

        install(&mut config, &entry.id, false).unwrap();
        assert_eq!(status(&config, entry), AgentCatalogStatus::Installed);
        assert_eq!(config.agents.len(), 1);
        assert_eq!(config.agents[0].persona, entry.persona);
        assert_eq!(config.agents[0].role.as_deref(), Some(entry.role.as_str()));

        // Installing again changes nothing. What a person changes in its powers does not make it "modified".
        install(&mut config, &entry.id, false).unwrap();
        assert_eq!(config.agents.len(), 1);
        config.agents[0].autonomy = 4;
        config.agents[0].can_manage_tasks = true;
        assert_eq!(status(&config, entry), AgentCatalogStatus::Installed);

        // Editing the text does.
        config.agents[0].persona = "My own words.".into();
        assert_eq!(status(&config, entry), AgentCatalogStatus::Modified);
    }

    #[test]
    fn installing_over_an_agent_that_differs_needs_overwrite_and_keeps_what_the_person_set() {
        let mut config = FileConfig::default();
        let id = entries()[0].id.clone();
        install(&mut config, &id, false).unwrap();
        config.agents[0].persona = "My own words.".into();
        config.agents[0].autonomy = 4;
        config.agents[0].reports_to = Some("boss".into());
        config.agents[0].shared_with = vec!["ana".into()];
        config.agents[0].provider_id = Some("main".into());

        let refused = install(&mut config, &id, false).unwrap_err();
        assert!(refused.contains("differs"), "{refused}");
        assert_eq!(config.agents[0].persona, "My own words.");

        install(&mut config, &id, true).unwrap();
        assert_eq!(config.agents[0].persona, entries()[0].persona);
        assert_eq!(config.agents[0].autonomy, 4);
        assert_eq!(config.agents[0].reports_to.as_deref(), Some("boss"));
        assert_eq!(config.agents[0].shared_with, ["ana"]);
        assert_eq!(config.agents[0].provider_id.as_deref(), Some("main"));
    }

    #[test]
    fn a_members_agent_with_the_same_id_is_never_replaced_and_an_unknown_id_is_an_error() {
        let mut config = FileConfig::default();
        let id = entries()[0].id.clone();
        install(&mut config, &id, false).unwrap();
        config.agents[0].owner = Some("ana".into());
        config.agents[0].persona = "Ana's.".into();

        assert_eq!(status(&config, &entries()[0]), AgentCatalogStatus::Modified);
        for overwrite in [false, true] {
            assert!(install(&mut config, &id, overwrite).unwrap_err().contains("member"));
        }
        assert_eq!(config.agents[0].persona, "Ana's.");

        assert!(install(&mut FileConfig::default(), "nope", false).unwrap_err().contains("no agent named"));
    }

    #[test]
    fn the_skills_an_agent_works_with_go_in_the_vault_without_replacing_one_the_person_has() {
        let store = temp_store();
        let agent = entries().iter().find(|e| e.skills.len() > 1).expect("an agent that works with several skills");
        let (first, rest) = agent.skills.split_first().unwrap();
        assert!(skills_kept(&store, agent).is_empty(), "an empty vault keeps nothing");

        // The person already has a different skill under the first name.
        store.save(&warden_core::skill::Skill::parse(first, "---\ndescription: Mine\n---\nMy own steps.")).unwrap();
        assert_eq!(skills_kept(&store, agent), [first.clone()], "the gallery says so before the install");

        let done = install_skills(&store, agent);
        assert_eq!(done.kept, [first.clone()]);
        assert_eq!(done.installed, rest);
        assert_eq!(store.get(first).unwrap().body, "My own steps.", "a skill of the person's is never replaced");
        for name in rest {
            assert!(store.exists(name), "{name}");
        }

        // Again: nothing new, and the one that differs is still reported.
        let again = install_skills(&store, agent);
        assert!(again.installed.is_empty());
        assert_eq!(again.kept, [first.clone()]);
        assert_eq!(skills_kept(&store, agent), [first.clone()], "and after it");
    }
}
