//! Slash-command parsing for the interactive REPL (`interactive.rs`) — pure text-in, typed-value-
//! out, with no `ratatui`/terminal dependency, so it's unit-testable the same way `LineEditor` is.

use warden_bootstrap::Provider;

/// How many calls `/ssh log` shows without a number, and the most it shows (what the screens read too).
const SSH_LOG_DEFAULT: usize = 20;
const SSH_LOG_MAX: usize = warden_bootstrap::ssh_audit::SSH_AUDIT_LIMIT;

/// One recognized slash-command, already split into its typed arguments.
pub enum Command {
    Exit,
    Help,
    /// `/lang` shows the language, `/lang <en|pt-BR>` switches it (P136). The value is checked by the handler, not here.
    Lang(Option<String>),
    Usage,
    /// `/folder` — the folder this session works in, if one was picked (P102).
    FolderShow,
    /// `/folder <path>` — works in that folder of this computer: set before the first message, never after.
    FolderSet(String),
    /// `/folder off` — no folder.
    FolderOff,
    /// `/limits` — where every spending limit stands (P4).
    Limits,
    /// `/extend <id>` — lets a limit go one step further for the rest of its window.
    Extend(String),
    /// `/grants` — what was answered "always" in this terminal (P122).
    GrantsList,
    /// `/grants revoke <covers> [agent]` — takes one "always" back. `agent` is `None` for the active agent and `Some("")` for the default agent (`padrao`).
    GrantsRevoke { covers: String, agent: Option<String> },
    LimitsAdd,
    LimitsEdit(String),
    LimitsRemove(String),
    /// `/limits off` — turns every spending limit off (`limits = []`).
    LimitsOff,
    /// `/limits reset` — drops back to the built-in safety net (no `[[limits]]` in the file).
    LimitsReset,
    PricesList,
    PricesAdd,
    PricesEdit(String),
    PricesRemove(String),
    ModelsList,
    ModelsUse(String),
    ModelsReset,
    ModelsAdd,
    ModelsEdit(String),
    ModelsRemove(String),
    CombosList,
    /// `/combos add <id> <provider>...` — the providers in order (P90).
    CombosAdd(String, Vec<String>),
    CombosRemove(String),
    AgentsList,
    /// `None` is `/agents use none` — clears the session's active agent.
    AgentsUse(Option<String>),
    /// `/agents tree` — who reports to whom (P120).
    AgentsTree,
    AgentsCreate,
    AgentsEdit(String),
    AgentsRemove(String),
    SkillsList,
    SkillsShow(String),
    SkillsCreate,
    SkillsEdit(String),
    SkillsRemove(String),
    /// `None` is a bare `/skills path` — the skills directory itself rather than one skill's file.
    SkillsPath(Option<String>),
    /// `/skills file <skill> <file>` — prints one attachment (P72 d).
    SkillsFile(String, String),
    /// `/skills attach <skill> <file> <local path>` — copies a local text file into the skill.
    SkillsAttach { skill: String, file: String, source: String },
    SkillsDetach(String, String),
    SshList,
    SshAdd,
    SshEdit(String),
    SshRemove(String),
    /// `/ssh on <id>` / `/ssh off <id>` — flips the master switch that lets the AI use a host.
    SshEnable(String, bool),
    SshTest(String),
    /// `/ssh log [n]` — the last `n` calls the SSH tools made (P47), read from `ssh_audit.jsonl`.
    SshLog(usize),
    SyncStatus,
    SyncPush,
    SyncPull,
    SyncPairShow,
    /// `/sync pair <code> [ip]` — with the ip, only that address is tried (a hub on a VPS over
    /// Tailscale, P88); without it, the LAN is swept.
    SyncPairJoin(String, Option<String>),
    SyncGitPush,
    SyncGitPull,
    /// `/arca` — whether this vault is in arca mode and whether its MCP logins travel.
    ArcaStatus,
    /// `/arca on` — turns the arca mode on for this vault (arca ADR 0012). One way only, so it asks to confirm.
    ArcaOn,
    /// `/arca sync` — one cycle against a hub; the address and the token are asked for, like in the pairing.
    ArcaSync,
    /// `/arca ack` — acknowledges the newer key epochs `/arca` lists (P-49 A2): this device keeps its key and syncing goes on. Asks to confirm.
    ArcaAck,
    /// `/arca swap grant` — on the device that rotated the key: hands the new key to a device still on the old one (P-49 C2).
    ArcaSwapGrant,
    /// `/arca swap offer` — on the device still on the old key: replaces it with the new one. Asks to confirm.
    ArcaSwapOffer,
    /// `/arca rotate` — after losing a device: makes a new vault key, shows its 24 words once, asks for 3 of them back and the
    /// declaration that the lost device's token was revoked on the hub, then rotates (P-49, arca ADR 0018, Warden ADR 0002).
    ArcaRotate,
    /// `/arca rotate resume` — finishes a rotation that stopped half way: the same 24 words are typed again, no new key is made.
    ArcaRotateResume,
    /// `/arca pair offer` — on the new device: offers itself to the hub and waits for the key (arca ADR 0016).
    ArcaPairOffer,
    /// `/arca pair grant` — on the device that has the key: hands it to a waiting offer.
    ArcaPairGrant,
}

/// What a line of input turned out to be, once checked against the slash-command grammar.
pub enum ParseOutcome {
    /// Doesn't start with `/` at all — treat as a normal chat message.
    NotACommand,
    Recognized(Command),
    /// Starts with `/` but doesn't match any known command/argument shape — must never be
    /// forwarded to the model as a chat message; the caller renders an error instead.
    Unrecognized(String),
}

pub fn parse_command(input: &str) -> ParseOutcome {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return ParseOutcome::NotACommand;
    }

    let mut parts = trimmed[1..].split_whitespace();
    let Some(head) = parts.next() else {
        return ParseOutcome::Unrecognized(trimmed.to_string());
    };
    let rest: Vec<&str> = parts.collect();

    let command = match (head.to_ascii_lowercase().as_str(), rest.as_slice()) {
        ("exit", []) | ("quit", []) => Command::Exit,
        ("help", []) => Command::Help,
        ("lang", []) => Command::Lang(None),
        ("lang", [value]) => Command::Lang(Some(value.to_string())),
        ("usage", []) => Command::Usage,
        ("folder", []) => Command::FolderShow,
        ("folder", ["off"]) => Command::FolderOff,
        // The path is the rest of the line, so a folder with spaces in its name works.
        ("folder", path) => Command::FolderSet(path.join(" ")),
        ("limits", []) => Command::Limits,
        ("limits", ["add"]) => Command::LimitsAdd,
        ("limits", ["edit", id]) => Command::LimitsEdit(id.to_string()),
        ("limits", ["remove", id]) => Command::LimitsRemove(id.to_string()),
        ("limits", ["off"]) => Command::LimitsOff,
        ("limits", ["reset"]) => Command::LimitsReset,
        ("extend", [id]) => Command::Extend(id.to_string()),
        ("grants", []) => Command::GrantsList,
        ("grants", ["revoke", covers]) => Command::GrantsRevoke { covers: covers.to_string(), agent: None },
        ("grants", ["revoke", covers, agent]) => Command::GrantsRevoke { covers: covers.to_string(), agent: Some(if *agent == DEFAULT_AGENT_WORD { String::new() } else { agent.to_string() }) },
        ("prices", []) => Command::PricesList,
        ("prices", ["add"]) => Command::PricesAdd,
        ("prices", ["edit", model]) => Command::PricesEdit(model.to_string()),
        ("prices", ["remove", model]) => Command::PricesRemove(model.to_string()),
        ("models", []) => Command::ModelsList,
        ("models", ["use", id]) => Command::ModelsUse(id.to_string()),
        ("models", ["reset"]) => Command::ModelsReset,
        ("models", ["add"]) => Command::ModelsAdd,
        ("models", ["edit", id]) => Command::ModelsEdit(id.to_string()),
        ("models", ["remove", id]) => Command::ModelsRemove(id.to_string()),
        ("combos", []) => Command::CombosList,
        ("combos", ["add", id, members @ ..]) if !members.is_empty() => {
            Command::CombosAdd(id.to_string(), members.iter().map(|m| m.to_string()).collect())
        }
        ("combos", ["remove", id]) => Command::CombosRemove(id.to_string()),
        ("agents", []) => Command::AgentsList,
        ("agents", ["use", "none"]) => Command::AgentsUse(None),
        ("agents", ["use", id]) => Command::AgentsUse(Some(id.to_string())),
        ("agents", ["tree"]) => Command::AgentsTree,
        ("agents", ["create"]) => Command::AgentsCreate,
        ("agents", ["edit", id]) => Command::AgentsEdit(id.to_string()),
        ("agents", ["remove", id]) => Command::AgentsRemove(id.to_string()),
        ("skills", []) => Command::SkillsList,
        ("skills", ["show", name]) => Command::SkillsShow(name.to_string()),
        ("skills", ["create"]) => Command::SkillsCreate,
        ("skills", ["edit", name]) => Command::SkillsEdit(name.to_string()),
        ("skills", ["remove", name]) => Command::SkillsRemove(name.to_string()),
        ("skills", ["path"]) => Command::SkillsPath(None),
        ("skills", ["path", name]) => Command::SkillsPath(Some(name.to_string())),
        ("skills", ["file", name, file]) => Command::SkillsFile(name.to_string(), file.to_string()),
        // Whitespace inside the path is collapsed by the tokenizer above; a path with a run of
        // spaces isn't supported, a single space is.
        ("skills", ["attach", name, file, source @ ..]) if !source.is_empty() => {
            Command::SkillsAttach { skill: name.to_string(), file: file.to_string(), source: source.join(" ") }
        }
        ("skills", ["detach", name, file]) => Command::SkillsDetach(name.to_string(), file.to_string()),
        ("ssh", []) => Command::SshList,
        ("ssh", ["add"]) => Command::SshAdd,
        ("ssh", ["edit", id]) => Command::SshEdit(id.to_string()),
        ("ssh", ["remove", id]) => Command::SshRemove(id.to_string()),
        ("ssh", ["on", id]) => Command::SshEnable(id.to_string(), true),
        ("ssh", ["off", id]) => Command::SshEnable(id.to_string(), false),
        ("ssh", ["test", id]) => Command::SshTest(id.to_string()),
        ("ssh", ["log"]) => Command::SshLog(SSH_LOG_DEFAULT),
        ("ssh", ["log", count]) if count.parse::<usize>().is_ok_and(|n| n > 0) => Command::SshLog(count.parse::<usize>().unwrap_or(SSH_LOG_DEFAULT).min(SSH_LOG_MAX)),
        ("sync", []) => Command::SyncStatus,
        ("sync", ["push"]) => Command::SyncPush,
        ("sync", ["pull"]) => Command::SyncPull,
        ("sync", ["pair"]) => Command::SyncPairShow,
        ("sync", ["pair", code]) => Command::SyncPairJoin(code.to_string(), None),
        ("sync", ["pair", code, host]) => Command::SyncPairJoin(code.to_string(), Some(host.to_string())),
        ("sync", ["git", "push"]) => Command::SyncGitPush,
        ("sync", ["git", "pull"]) => Command::SyncGitPull,
        ("arca", []) => Command::ArcaStatus,
        ("arca", ["on"]) => Command::ArcaOn,
        ("arca", ["sync"]) => Command::ArcaSync,
        ("arca", ["ack"]) => Command::ArcaAck,
        ("arca", ["swap", "grant"]) => Command::ArcaSwapGrant,
        ("arca", ["swap", "offer"]) => Command::ArcaSwapOffer,
        ("arca", ["rotate"]) => Command::ArcaRotate,
        ("arca", ["rotate", "resume"]) => Command::ArcaRotateResume,
        ("arca", ["pair", "offer"]) => Command::ArcaPairOffer,
        ("arca", ["pair", "grant"]) => Command::ArcaPairGrant,
        _ => return ParseOutcome::Unrecognized(trimmed.to_string()),
    };
    ParseOutcome::Recognized(command)
}

/// Whether `input` starts `/arca rotate` and is not exactly `/arca rotate` or `/arca rotate resume`: the person typed something
/// after it, and for `resume` that something is probably the 24 words. The parser refuses it (arguments are never read from the
/// line), but the REPL must also keep that line out of the line history (saved in clear text) and out of the "unknown command"
/// card, which echoes the line: the caller asks this before doing either. Whitespace and the case of the first two words are
/// read the way `parse_command` reads them, and an `/arca ROTATE resume ...` that the parser does not know still counts.
pub fn is_rotate_with_arguments(input: &str) -> bool {
    let Some(rest) = input.trim().strip_prefix('/') else { return false };
    let mut words = rest.split_whitespace();
    let starts_arca_rotate = words.next().is_some_and(|head| head.eq_ignore_ascii_case("arca")) && words.next().is_some_and(|sub| sub.eq_ignore_ascii_case("rotate"));
    starts_arca_rotate && matches!(parse_command(input), ParseOutcome::Unrecognized(_))
}

/// A phrase has 24 words; no command of this grammar takes anywhere near 12 tokens that the parser then refuses.
const PHRASE_LINE_TOKENS: usize = 12;
/// 24 words of 3 letters or more are 95 characters even with a comma or a zero-width space between them: a refused command line of
/// 64 or more is not worth keeping either, and catches a phrase whose words are not separated by whitespace.
const PHRASE_LINE_CHARS: usize = 64;

/// Whether a submitted line may hold the 24 words, so the REPL must keep it out of the line history (saved in clear text) and must
/// not echo it. The rule does not trust the first words (`/arca rotat resume ...`, `/arca resume ...`, `/rotate ...`, a zero-width
/// space inside a word all slip past any list of prefixes): it is `/arca rotate ...` with arguments (the exact case the text of a
/// stop half way sends people to), or any line that starts with `/`, that the parser refuses, and that is as long as a phrase
/// (12 or more whitespace-separated tokens, or 64 or more characters). A command the parser knows never counts, however long, and
/// neither does chat (no slash: whether chat should be guarded is the owner's decision, not this rule's).
pub fn may_hold_a_phrase(input: &str) -> bool {
    if is_rotate_with_arguments(input) {
        return true;
    }
    let trimmed = input.trim();
    trimmed.starts_with('/')
        && matches!(parse_command(input), ParseOutcome::Unrecognized(_))
        && (trimmed.split_whitespace().count() >= PHRASE_LINE_TOKENS || trimmed.chars().count() >= PHRASE_LINE_CHARS)
}

/// Whether a submitted line is, by itself, the recovery phrase (a chat line with no slash). Skeleton: always `false` for now.
pub fn is_bare_phrase(_input: &str) -> bool {
    false
}

/// Parses a provider `kind` the way a user would type it (matching the config file's own
/// `#[serde(rename_all = "snake_case")]` spelling) — there's no `FromStr` on `Provider` itself
/// since its only other consumer (`toml`) goes through `serde`, not `str::parse`.
pub fn parse_provider_kind(input: &str) -> Option<Provider> {
    match input.trim().to_ascii_lowercase().as_str() {
        "gemini" => Some(Provider::Gemini),
        "openai" => Some(Provider::Openai),
        "anthropic" => Some(Provider::Anthropic),
        "openai_compatible" | "openai-compatible" => Some(Provider::OpenaiCompatible),
        "cloud" => Some(Provider::Cloud),
        _ => None,
    }
}

/// The reverse of `parse_provider_kind` — used to display a provider's kind back to the user.
pub fn kind_label(kind: Provider) -> &'static str {
    match kind {
        Provider::Gemini => "gemini",
        Provider::Openai => "openai",
        Provider::Anthropic => "anthropic",
        Provider::OpenaiCompatible => "openai_compatible",
        Provider::Node => "node",
        Provider::Cloud => "cloud",
    }
}

/// What `/grants revoke <covers> padrao` calls the default agent (the one with no id).
pub const DEFAULT_AGENT_WORD: &str = "padrao";

const GRANTS_SUBCOMMANDS: &[&str] = &["revoke"];
const TOP_LEVEL_COMMANDS: &[&str] = &["exit", "quit", "help", "lang", "usage", "limits", "extend", "grants", "prices", "models", "combos", "agents", "skills", "ssh", "sync", "arca"];
const LIMITS_SUBCOMMANDS: &[&str] = &["add", "edit", "remove", "off", "reset"];
const PRICES_SUBCOMMANDS: &[&str] = &["add", "edit", "remove"];
const MODELS_SUBCOMMANDS: &[&str] = &["use", "reset", "add", "edit", "remove"];
const COMBOS_SUBCOMMANDS: &[&str] = &["add", "remove"];
const AGENTS_SUBCOMMANDS: &[&str] = &["use", "tree", "create", "edit", "remove"];
const SKILLS_SUBCOMMANDS: &[&str] = &["show", "create", "edit", "remove", "path", "file", "attach", "detach"];
const SSH_SUBCOMMANDS: &[&str] = &["add", "edit", "remove", "on", "off", "test"];
const SYNC_SUBCOMMANDS: &[&str] = &["push", "pull", "pair", "git"];
const ARCA_SUBCOMMANDS: &[&str] = &["on", "sync", "ack", "swap", "rotate", "pair"];

/// Parses the word currently being typed (the last whitespace-separated token) out of a
/// `/`-prefixed `input`, along with its candidate completions from the fixed part of the grammar
/// (`parse_command`'s own vocabulary — command names one level, subcommand names the next).
/// Returns `None` once a subcommand that takes a free-form argument (an id, a persona, ...) has
/// already been typed — there's no fixed list to suggest there, and this grammar never branches
/// again past the second word. `ghost_suggestion` (its only caller) needs the exact typed
/// fragment as well as the candidates, to compute the completion suffix.
fn current_word(input: &str) -> Option<(&str, Vec<&'static str>)> {
    let after_slash = input.strip_prefix('/')?;
    if let Some(space_idx) = after_slash.find(' ') {
        let head = after_slash[..space_idx].to_ascii_lowercase();
        let rest = &after_slash[space_idx + 1..];
        if rest.contains(' ') {
            return None;
        }
        let subcommands: &[&str] = match head.as_str() {
            "limits" => LIMITS_SUBCOMMANDS,
            "grants" => GRANTS_SUBCOMMANDS,
            "prices" => PRICES_SUBCOMMANDS,
            "models" => MODELS_SUBCOMMANDS,
            "combos" => COMBOS_SUBCOMMANDS,
            "agents" => AGENTS_SUBCOMMANDS,
            "skills" => SKILLS_SUBCOMMANDS,
            "ssh" => SSH_SUBCOMMANDS,
            "sync" => SYNC_SUBCOMMANDS,
            "arca" => ARCA_SUBCOMMANDS,
            _ => return None,
        };
        let typed_lower = rest.to_ascii_lowercase();
        Some((rest, subcommands.iter().copied().filter(|c| c.starts_with(typed_lower.as_str())).collect()))
    } else {
        let typed_lower = after_slash.to_ascii_lowercase();
        Some((after_slash, TOP_LEVEL_COMMANDS.iter().copied().filter(|c| c.starts_with(typed_lower.as_str())).collect()))
    }
}

/// The live "ghost" completion to show/accept for `input` (the whole line typed so far) — the
/// remaining characters that would complete its last word to a single unambiguous candidate.
/// `None` when there's no `/`-command context, the word is already fully typed, or more than one
/// candidate remains (ambiguous — nothing single to preview or accept on Tab).
pub fn ghost_suggestion(input: &str) -> Option<String> {
    let (typed, candidates) = current_word(input)?;
    if candidates.len() != 1 {
        return None;
    }
    let only = candidates[0];
    if only.len() <= typed.len() {
        return None;
    }
    Some(only[typed.len()..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_recognized(input: &str) -> Command {
        match parse_command(input) {
            ParseOutcome::Recognized(command) => command,
            ParseOutcome::NotACommand => panic!("expected a command, got NotACommand for {input:?}"),
            ParseOutcome::Unrecognized(_) => panic!("expected a command, got Unrecognized for {input:?}"),
        }
    }

    #[test]
    fn plain_text_is_not_a_command() {
        assert!(matches!(parse_command("hello there"), ParseOutcome::NotACommand));
        assert!(matches!(parse_command(""), ParseOutcome::NotACommand));
    }

    #[test]
    fn unrecognized_slash_input_is_never_treated_as_chat() {
        assert!(matches!(parse_command("/nonsense"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/models bogus"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/models use"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn ssh_commands_parse_with_their_arguments() {
        assert!(matches!(assert_recognized("/ssh"), Command::SshList));
        assert!(matches!(assert_recognized("/ssh add"), Command::SshAdd));
        assert!(matches!(assert_recognized("/ssh edit vps"), Command::SshEdit(id) if id == "vps"));
        assert!(matches!(assert_recognized("/ssh remove vps"), Command::SshRemove(id) if id == "vps"));
        assert!(matches!(assert_recognized("/ssh on vps"), Command::SshEnable(id, true) if id == "vps"));
        assert!(matches!(assert_recognized("/ssh off vps"), Command::SshEnable(id, false) if id == "vps"));
        assert!(matches!(assert_recognized("/ssh test vps"), Command::SshTest(id) if id == "vps"));
        assert!(matches!(assert_recognized("/ssh log"), Command::SshLog(20)));
        assert!(matches!(assert_recognized("/ssh log 5"), Command::SshLog(5)));
        assert!(matches!(assert_recognized("/ssh log 5000"), Command::SshLog(100)), "never more than the screens show");
        assert!(matches!(parse_command("/ssh log 0"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/ssh log many"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/ssh on"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/ssh bogus vps"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn exit_and_quit_and_help_parse_with_no_args() {
        assert!(matches!(assert_recognized("/exit"), Command::Exit));
        assert!(matches!(assert_recognized("/quit"), Command::Exit));
        assert!(matches!(assert_recognized("/help"), Command::Help));
        assert!(matches!(assert_recognized("/usage"), Command::Usage));
    }

    #[test]
    fn lang_shows_or_sets_the_language() {
        assert!(matches!(assert_recognized("/lang"), Command::Lang(None)));
        assert!(matches!(assert_recognized("/lang pt-BR"), Command::Lang(Some(value)) if value == "pt-BR"));
        assert!(matches!(parse_command("/lang en pt"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn limits_and_extend_parse_and_extend_needs_a_limit_id() {
        assert!(matches!(assert_recognized("/limits"), Command::Limits));
        assert!(matches!(assert_recognized("/extend default-day"), Command::Extend(id) if id == "default-day"));
        assert!(matches!(parse_command("/extend"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/limits now"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn grants_lists_and_revokes_by_what_it_covers_for_the_active_agent_or_a_named_one() {
        assert!(matches!(assert_recognized("/grants"), Command::GrantsList));
        assert!(matches!(assert_recognized("/grants revoke shell"), Command::GrantsRevoke { covers, agent: None } if covers == "shell"));
        assert!(matches!(assert_recognized("/grants revoke delete_data ana"), Command::GrantsRevoke { covers, agent: Some(a) } if covers == "delete_data" && a == "ana"));
        assert!(matches!(assert_recognized("/grants revoke shell padrao"), Command::GrantsRevoke { agent: Some(a), .. } if a.is_empty()));
        assert!(matches!(parse_command("/grants revoke"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/grants revoke a b c"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/grants bogus"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn limits_subcommands_parse_their_arguments() {
        assert!(matches!(assert_recognized("/limits add"), Command::LimitsAdd));
        assert!(matches!(assert_recognized("/limits edit daily"), Command::LimitsEdit(id) if id == "daily"));
        assert!(matches!(assert_recognized("/limits remove daily"), Command::LimitsRemove(id) if id == "daily"));
        assert!(matches!(assert_recognized("/limits off"), Command::LimitsOff));
        assert!(matches!(assert_recognized("/limits reset"), Command::LimitsReset));
        assert!(matches!(parse_command("/limits edit"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/limits bogus"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn prices_subcommands_parse_their_arguments() {
        assert!(matches!(assert_recognized("/prices"), Command::PricesList));
        assert!(matches!(assert_recognized("/prices add"), Command::PricesAdd));
        assert!(matches!(assert_recognized("/prices edit gpt-4o-mini"), Command::PricesEdit(m) if m == "gpt-4o-mini"));
        assert!(matches!(assert_recognized("/prices remove gpt-4o-mini"), Command::PricesRemove(m) if m == "gpt-4o-mini"));
        assert!(matches!(parse_command("/prices edit"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/prices bogus"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn models_subcommands_parse_their_arguments() {
        assert!(matches!(assert_recognized("/models"), Command::ModelsList));
        assert!(matches!(assert_recognized("/models reset"), Command::ModelsReset));
        assert!(matches!(assert_recognized("/models add"), Command::ModelsAdd));
        assert!(matches!(assert_recognized("/models use my-id"), Command::ModelsUse(id) if id == "my-id"));
        assert!(matches!(assert_recognized("/models edit my-id"), Command::ModelsEdit(id) if id == "my-id"));
        assert!(matches!(assert_recognized("/models remove my-id"), Command::ModelsRemove(id) if id == "my-id"));
        assert!(matches!(assert_recognized("/combos"), Command::CombosList));
        assert!(matches!(assert_recognized("/combos add fast gemini ollama"), Command::CombosAdd(id, m) if id == "fast" && m == ["gemini", "ollama"]));
        assert!(matches!(assert_recognized("/combos remove fast"), Command::CombosRemove(id) if id == "fast"));
    }

    #[test]
    fn skills_subcommands_parse_their_arguments() {
        assert!(matches!(assert_recognized("/skills"), Command::SkillsList));
        assert!(matches!(assert_recognized("/skills create"), Command::SkillsCreate));
        assert!(matches!(assert_recognized("/skills show review-pr"), Command::SkillsShow(n) if n == "review-pr"));
        assert!(matches!(assert_recognized("/skills edit review-pr"), Command::SkillsEdit(n) if n == "review-pr"));
        assert!(matches!(assert_recognized("/skills remove review-pr"), Command::SkillsRemove(n) if n == "review-pr"));
        assert!(matches!(assert_recognized("/skills path"), Command::SkillsPath(None)));
        assert!(matches!(assert_recognized("/skills path review-pr"), Command::SkillsPath(Some(n)) if n == "review-pr"));
        assert!(matches!(assert_recognized("/skills file review-pr run.sh"), Command::SkillsFile(n, f) if n == "review-pr" && f == "run.sh"));
        assert!(matches!(assert_recognized("/skills detach review-pr run.sh"), Command::SkillsDetach(n, f) if n == "review-pr" && f == "run.sh"));
        assert!(matches!(
            assert_recognized("/skills attach review-pr run.sh ./scripts/my run.sh"),
            Command::SkillsAttach { skill, file, source } if skill == "review-pr" && file == "run.sh" && source == "./scripts/my run.sh"
        ));
        assert!(matches!(parse_command("/skills attach review-pr run.sh"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/skills file review-pr"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/skills show"), ParseOutcome::Unrecognized(_)));
        assert!(matches!(parse_command("/skills bogus"), ParseOutcome::Unrecognized(_)));
    }

    #[test]
    fn agents_use_none_clears_while_use_with_an_id_sets_it() {
        assert!(matches!(assert_recognized("/agents use none"), Command::AgentsUse(None)));
        assert!(matches!(assert_recognized("/agents use pirata"), Command::AgentsUse(Some(id)) if id == "pirata"));
    }

    #[test]
    fn agents_tree_shows_the_organization() {
        assert!(matches!(assert_recognized("/agents tree"), Command::AgentsTree));
    }

    #[test]
    fn sync_subcommands_parse_their_arguments() {
        assert!(matches!(assert_recognized("/sync"), Command::SyncStatus));
        assert!(matches!(assert_recognized("/sync push"), Command::SyncPush));
        assert!(matches!(assert_recognized("/sync pull"), Command::SyncPull));
        assert!(matches!(assert_recognized("/sync pair"), Command::SyncPairShow));
    }

    #[test]
    fn sync_pair_join_captures_the_typed_code() {
        assert!(matches!(assert_recognized("/sync pair ABCD1234"), Command::SyncPairJoin(code, None) if code == "ABCD1234"));
        assert!(matches!(
            assert_recognized("/sync pair ABCD1234 100.64.0.2"),
            Command::SyncPairJoin(code, Some(host)) if code == "ABCD1234" && host == "100.64.0.2"
        ));
    }

    #[test]
    fn sync_git_subcommands_parse() {
        assert!(matches!(assert_recognized("/sync git push"), Command::SyncGitPush));
        assert!(matches!(assert_recognized("/sync git pull"), Command::SyncGitPull));
    }

    #[test]
    fn arca_pair_subcommands_parse_and_take_no_arguments() {
        assert!(matches!(assert_recognized("/arca pair offer"), Command::ArcaPairOffer));
        assert!(matches!(assert_recognized("/arca pair grant"), Command::ArcaPairGrant));
        // The hub address and the token are asked for in the terminal, never taken from the line (it would stay in the history).
        for line in ["/arca pair", "/arca pair offer https://hub", "/arca pair grant token123", "/arca pair other"] {
            assert!(matches!(parse_command(line), ParseOutcome::Unrecognized(_)), "{line}");
        }
    }

    #[test]
    fn arca_status_sync_and_on_parse_and_take_no_arguments() {
        assert!(matches!(assert_recognized("/arca"), Command::ArcaStatus));
        assert!(matches!(assert_recognized("/arca sync"), Command::ArcaSync));
        assert!(matches!(assert_recognized("/arca on"), Command::ArcaOn));
        // Same rule as the pairing: the hub address and the token are asked for in the terminal, never taken from the line.
        // `/arca on` takes nothing either (no `--yes`): the confirmation is always typed, and there is no `/arca off`.
        for line in ["/arca sync https://hub", "/arca sync token123", "/arca status", "/arca on now", "/arca on --yes", "/arca off"] {
            assert!(matches!(parse_command(line), ParseOutcome::Unrecognized(_)), "{line}");
        }
    }

    #[test]
    fn arca_ack_parses_and_takes_no_arguments() {
        assert!(matches!(assert_recognized("/arca ack"), Command::ArcaAck));
        // What is acknowledged is chosen on the screen from the list the person just read, never typed on the line.
        for line in ["/arca ack 1", "/arca ack all", "/arca ack --yes", "/arca acknowledge"] {
            assert!(matches!(parse_command(line), ParseOutcome::Unrecognized(_)), "{line}");
        }
    }

    #[test]
    fn arca_swap_parses_and_takes_no_arguments() {
        assert!(matches!(assert_recognized("/arca swap grant"), Command::ArcaSwapGrant));
        assert!(matches!(assert_recognized("/arca swap offer"), Command::ArcaSwapOffer));
        // Same rule as the pairing: the hub address and the token are asked for in the terminal, never taken from the line.
        for line in ["/arca swap", "/arca swap grant https://hub", "/arca swap offer token123", "/arca swap now", "/arca swap offer --yes"] {
            assert!(matches!(parse_command(line), ParseOutcome::Unrecognized(_)), "{line}");
        }
    }

    #[test]
    fn arca_rotate_and_its_resume_parse_and_take_no_arguments() {
        assert!(matches!(assert_recognized("/arca rotate"), Command::ArcaRotate));
        assert!(matches!(assert_recognized("/arca rotate resume"), Command::ArcaRotateResume));
        // The tokenizer collapses whitespace, so a run of spaces between the words is the same command.
        assert!(matches!(assert_recognized("  /arca   rotate    resume  "), Command::ArcaRotateResume));
        // No argument is ever taken from the line: this only proves the PARSER refuses them. What keeps a phrase typed there out
        // of the line history and out of the "unknown command" card is `is_rotate_with_arguments` (tested below) and the
        // REPL's use of it (`remember_line`, `unknown_command_lines` in `interactive.rs`).
        let phrase_on_the_line = format!("/arca rotate resume {}", ["zoo"; 24].join(" "));
        for line in [
            "/arca rotate now",
            "/arca rotate --yes",
            "/arca rotate revoked",
            "/arca rotate resume now",
            "/arca rotate resume --yes",
            "/arca rotate resume abandon",
            "/arca rotate resume resume",
            "/arca resume",
            "/arca rotate rotate",
            phrase_on_the_line.as_str(),
        ] {
            assert!(matches!(parse_command(line), ParseOutcome::Unrecognized(_)), "{line}");
        }
    }

    #[test]
    fn a_rotate_line_with_arguments_is_told_apart_from_the_two_exact_commands() {
        // The phrase of the official BIP-39 vector for 32 bytes of 0xff (arca-core `recovery.rs`): a real one, not "word word".
        let phrase = format!("{} vote", ["zoo"; 23].join(" "));
        let with_phrase = format!("/arca rotate resume {phrase}");
        let shouty = format!("/ARCA rotate resume {phrase}");
        let upper_sub = format!("/arca ROTATE resume {phrase}");
        let slash_space = format!("/ arca rotate resume {phrase}");
        let padded = format!("   /arca   rotate   resume   {phrase}   ");
        for line in [
            with_phrase.as_str(),
            shouty.as_str(),
            upper_sub.as_str(),
            slash_space.as_str(),
            padded.as_str(),
            "/arca rotate resume abandon",
            "/arca rotate now",
            "/arca rotate --yes",
            "/arca rotate rotate",
        ] {
            assert!(is_rotate_with_arguments(line), "{line}");
        }
        for line in ["/arca rotate", "/arca rotate resume", "  /arca   rotate   resume  ", "/arca sync", "/arca swap now", "/arca rotated", "/arca", "/nonsense", "hello rotate", "arca rotate resume x", ""] {
            assert!(!is_rotate_with_arguments(line), "{line}: only a line that starts /arca rotate and is not one of the two commands");
        }
    }

    #[test]
    fn any_unknown_slash_line_that_may_hold_a_phrase_is_told_apart_whatever_its_first_words() {
        // The phrase of the official BIP-39 vector for 32 bytes of 0xff (arca-core `recovery.rs`): a real one.
        let phrase = format!("{} vote", ["zoo"; 23].join(" "));
        let zero_width_joined = phrase.replace(' ', "\u{200B}");
        let comma_joined = phrase.replace(' ', ",");
        let lines = [
            format!("/arca rotat resume {phrase}"),
            format!("/arca rotatex {phrase}"),
            format!("/arca resume {phrase}"),
            format!("/arca rotate-resume {phrase}"),
            format!("/rotate resume {phrase}"),
            format!("/arca sync {phrase}"),
            format!("/arca\u{200B}rotate resume {phrase}"),
            format!("/arca rotate\u{200B}resume {phrase}"),
            // No whitespace between the words at all: one token, but far too long for a command.
            format!("/arca rotate resume {zero_width_joined}"),
            format!("/arca rotat resume {zero_width_joined}"),
            format!("/arca rotat resume {comma_joined}"),
            // The exact forms the older rule already knew.
            format!("/arca rotate resume {phrase}"),
            format!("  /ARCA  rotate  resume  {phrase}  "),
        ];
        for line in &lines {
            assert!(may_hold_a_phrase(line), "a line of {} characters and {} tokens was not caught", line.chars().count(), line.split_whitespace().count());
        }
        // 12 tokens in all is the edge of the token rule (a phrase has 24); these lines are short, so the length rule is not what decides.
        let ten_words = ["zoo"; 10].join(" ");
        let eleven_words = ["zoo"; 11].join(" ");
        assert!(!may_hold_a_phrase(&format!("/nonsense {ten_words}")), "11 tokens");
        assert!(may_hold_a_phrase(&format!("/nonsense {eleven_words}")), "12 tokens");
    }

    #[test]
    fn what_is_not_an_unknown_slash_line_or_is_short_is_left_alone() {
        let long_but_known = "/skills attach name file a b c d e f g h i j k l m n o p q r s t u v w x y z";
        assert!(matches!(parse_command(long_but_known), ParseOutcome::Recognized(_)), "the line really is a command");
        // A recognized command never counts, however long; nor does chat (no slash: that is the owner's product decision, not this rule).
        let chat = ["zoo"; 24].join(" ");
        for line in [long_but_known, chat.as_str(), "/arca rotate", "/arca rotate resume", "/nonsense", "/models bogus", "/nonsense now", "/ssh bogus vps", "", "   "] {
            assert!(!may_hold_a_phrase(line), "{line}");
        }
    }

    /// Three real BIP-39 phrases of 24 words, copied from the official vectors in arca-core `recovery.rs` (`VECTORS`, at the revision
    /// `warden-sync` pins; the first, 0xff, is the `ZOO_PHRASE` of the `interactive.rs` tests). Not one of them is the 0xff phrase only:
    /// a rule that matched just that one would pass with a fixture of "zoo".
    fn real_phrases() -> [String; 3] {
        [
            format!("{} vote", ["zoo"; 23].join(" ")),
            format!("{} art", ["abandon"; 23].join(" ")),
            "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title".to_string(),
        ]
    }

    #[test]
    fn a_chat_line_that_is_a_valid_phrase_is_told_apart_whatever_its_case_and_spacing() {
        for (index, phrase) in real_phrases().iter().enumerate() {
            // (The messages name the phrase by its index: a failing run must not put a phrase in the log.)
            let spaced_out = phrase.replace(' ', "   ");
            let tabbed = phrase.replace(' ', "\t");
            let lines = [
                phrase.clone(),
                format!("  {phrase}  "),
                format!("{phrase}\n"),
                phrase.to_uppercase(),
                format!("  {}  ", phrase.to_uppercase()),
                spaced_out,
                tabbed,
            ];
            for (variant, line) in lines.iter().enumerate() {
                assert!(is_bare_phrase(line), "phrase #{index}, variant #{variant} was not caught");
            }
        }
    }

    #[test]
    fn what_is_not_a_valid_phrase_on_its_own_is_left_to_the_chat() {
        let [zoo, abandon, legal] = real_phrases();
        // 24 words of the list, but not a phrase: the checksum word is wrong (the real one ends in "vote" / "art" / "title").
        let wrong_checksum = ["zoo"; 24].join(" ");
        let wrong_checksum_abandon = ["abandon"; 24].join(" ");
        let swapped_last_word = legal.replace("title", "zoo");
        // A valid phrase with one word less, or one more.
        let twenty_three_words = zoo.replacen(" vote", "", 1);
        let twenty_five_words = format!("{abandon} zoo");
        let phrase_in_a_sentence = format!("my phrase is {legal}");
        let phrase_with_a_question = format!("{zoo} ?");
        // A word that is not on the list at all.
        let unknown_word = zoo.replacen("vote", "votes", 1);
        let not_a_phrase = [
            wrong_checksum.as_str(),
            wrong_checksum_abandon.as_str(),
            swapped_last_word.as_str(),
            twenty_three_words.as_str(),
            twenty_five_words.as_str(),
            phrase_in_a_sentence.as_str(),
            phrase_with_a_question.as_str(),
            unknown_word.as_str(),
            "hello",
            "please explain what a zoo is and why the abandon rate matters",
            "",
            "   ",
        ];
        for (index, line) in not_a_phrase.iter().enumerate() {
            assert!(!is_bare_phrase(line), "line #{index} was blocked, but it is not a phrase on its own");
        }
        // A line with a slash is the business of `may_hold_a_phrase` and the parser, not of this rule (a "/zoo" is not a word of the list).
        for (index, phrase) in [&zoo, &abandon, &legal].into_iter().enumerate() {
            assert!(!is_bare_phrase(&format!("/{phrase}")), "phrase #{index} behind a slash");
            assert!(!is_bare_phrase(&format!("/arca rotate resume {phrase}")), "phrase #{index} after /arca rotate resume");
        }
    }

    #[test]
    fn ghost_suggestion_knows_the_arca_rotate_subcommand() {
        // `rotate` is the only `/arca` subcommand that starts with "r", so one letter is enough.
        assert_eq!(ghost_suggestion("/arca r").as_deref(), Some("otate"));
        assert_eq!(ghost_suggestion("/arca ro").as_deref(), Some("tate"));
        // The others are not disturbed: `swap` and `sync` still need their second letter.
        assert_eq!(ghost_suggestion("/arca s"), None);
        assert_eq!(ghost_suggestion("/arca sw").as_deref(), Some("ap"));
    }

    #[test]
    fn ghost_suggestion_knows_the_arca_swap_subcommand() {
        assert_eq!(ghost_suggestion("/arca sw").as_deref(), Some("ap"));
        assert_eq!(ghost_suggestion("/arca sy").as_deref(), Some("nc"));
    }

    #[test]
    fn ghost_suggestion_knows_the_arca_ack_subcommand() {
        assert_eq!(ghost_suggestion("/arca a").as_deref(), Some("ck"));
    }

    #[test]
    fn ghost_suggestion_knows_the_arca_on_subcommand() {
        assert_eq!(ghost_suggestion("/arca o").as_deref(), Some("n"));
    }

    #[test]
    fn ghost_suggestion_knows_the_arca_sync_subcommand() {
        // `swap` also starts with "s", so the ghost needs the second letter to tell the two apart.
        assert_eq!(ghost_suggestion("/arca sy").as_deref(), Some("nc"));
    }

    #[test]
    fn command_matching_is_case_insensitive_on_the_head_word() {
        assert!(matches!(assert_recognized("/EXIT"), Command::Exit));
        assert!(matches!(assert_recognized("/Models"), Command::ModelsList));
    }

    #[test]
    fn parse_provider_kind_matches_the_toml_snake_case_spelling() {
        assert_eq!(parse_provider_kind("gemini"), Some(Provider::Gemini));
        assert_eq!(parse_provider_kind("OpenAI".to_lowercase().as_str()), Some(Provider::Openai));
        assert_eq!(parse_provider_kind("openai_compatible"), Some(Provider::OpenaiCompatible));
        assert_eq!(parse_provider_kind("nonsense"), None);
    }

    #[test]
    fn kind_label_round_trips_through_parse_provider_kind() {
        for kind in [Provider::Gemini, Provider::Openai, Provider::Anthropic, Provider::OpenaiCompatible] {
            assert_eq!(parse_provider_kind(kind_label(kind)), Some(kind));
        }
    }

    fn candidates_for(input: &str) -> Vec<&'static str> {
        let mut candidates = current_word(input).map(|(_, candidates)| candidates).unwrap_or_default();
        candidates.sort_unstable();
        candidates
    }

    #[test]
    fn current_word_completes_the_top_level_command_name() {
        assert_eq!(candidates_for("/mo"), vec!["models"]);
        assert_eq!(candidates_for("/e"), vec!["exit", "extend"]);
    }

    #[test]
    fn current_word_completes_a_subcommand_name() {
        assert_eq!(candidates_for("/models u"), vec!["use"]);
        assert_eq!(candidates_for("/agents "), vec!["create", "edit", "remove", "tree", "use"]);
        assert_eq!(candidates_for("/skills "), vec!["attach", "create", "detach", "edit", "file", "path", "remove", "show"]);
        assert_eq!(candidates_for("/sync pu"), vec!["pull", "push"]);
        assert_eq!(candidates_for("/sync g"), vec!["git"]);
        assert_eq!(candidates_for("/limits "), vec!["add", "edit", "off", "remove", "reset"]);
        assert_eq!(candidates_for("/prices "), vec!["add", "edit", "remove"]);
    }

    #[test]
    fn current_word_has_nothing_once_past_the_subcommand_word() {
        assert!(candidates_for("/models use my-i").is_empty());
        assert!(candidates_for("plain text").is_empty());
        assert!(candidates_for("/bogus").is_empty());
    }

    #[test]
    fn ghost_suggestion_completes_an_unambiguous_top_level_word() {
        assert_eq!(ghost_suggestion("/mo").as_deref(), Some("dels"));
        assert_eq!(ghost_suggestion("/exi").as_deref(), Some("t"));
        assert_eq!(ghost_suggestion("/ext").as_deref(), Some("end"));
        assert_eq!(ghost_suggestion("/li").as_deref(), Some("mits"));
    }

    #[test]
    fn ghost_suggestion_completes_an_unambiguous_subcommand_word() {
        assert_eq!(ghost_suggestion("/models u").as_deref(), Some("se"));
        assert_eq!(ghost_suggestion("/agents c").as_deref(), Some("reate"));
    }

    #[test]
    fn ghost_suggestion_is_none_when_ambiguous_or_already_complete_or_out_of_grammar() {
        assert_eq!(ghost_suggestion("/"), None); // "" matches every top-level command — ambiguous
        assert_eq!(ghost_suggestion("/models"), None); // already a full, unique match
        assert_eq!(ghost_suggestion("/models use my-i"), None); // past the subcommand word
        assert_eq!(ghost_suggestion("plain text"), None);
    }
}
