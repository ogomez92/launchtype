//! Merging a second commands file into the current one.
//!
//! The feature is additive and nothing else. A merge can only ever append
//! commands the current file does not already have: nothing existing is
//! edited, re-pointed, renamed or deleted, and the user is never asked to
//! choose between two versions of the same command — an entry that is already
//! here simply never appears in the list.
//!
//! That constraint is what makes the flow safe to run against a 389-command
//! file you care about: the worst a careless merge can do is add rows you then
//! delete.

use std::collections::HashSet;

use crate::model::{Command, CommandsFile};
use crate::portable::{self, Target, Vars};

/// Why the file the user picked cannot be merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Not JSON at all — the wrong file was picked.
    NotJson,
    /// Valid JSON, but not a Launchtype commands document.
    NotCommandsFile,
}

/// Something worth knowing about an incoming command before ticking it.
///
/// A warning never blocks an import. The point of merging is to carry commands
/// between machines, and a path that is missing here may well be a program the
/// user is about to install; the check exists so nothing arrives as a silent
/// surprise, not to police what may come in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// The shortcut already belongs to a command that is staying put. Only the
    /// first command matching a shortcut ever runs, so [`merge`] drops it from
    /// the copy being imported rather than adding a dead one.
    ShortcutTaken { shortcut: String, owner: String },
    /// Two commands in the file being merged share a shortcut; the one listed
    /// first keeps it.
    ShortcutTakenInFile { shortcut: String, owner: String },
    /// A `{{name}}` this machine cannot resolve — a typo, or a variable from a
    /// newer version. It would be launched literally.
    UnknownVariable { name: String },
    /// The path resolves to a file that is not on this machine.
    MissingPath { path: String },
}

/// One incoming command the user can tick, with whatever the checks turned up.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub command: Command,
    pub warnings: Vec<Warning>,
}

/// What merging a given file would offer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// The commands not already present, in the order the merged file lists
    /// them.
    pub candidates: Vec<Candidate>,
    /// How many incoming commands were left out for being here already. Counts
    /// repeats inside the merged file itself too, so `candidates.len() +
    /// already_present` is always the number of commands that file holds.
    pub already_present: usize,
}

/// Read a commands document out of arbitrary JSON text.
///
/// `settings.json`, `timers.json` and `alarms.json` all sit in the same folder
/// and are all valid JSON, so the `"commands"` array is what identifies one of
/// ours — the same shape test Settings' commands-file picker uses (see
/// [`crate::model::commands_files_in`]).
pub fn parse(text: &str) -> Result<CommandsFile, ParseError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| ParseError::NotJson)?;
    if !value.get("commands").is_some_and(|commands| commands.is_array()) {
        return Err(ParseError::NotCommandsFile);
    }
    // Shaped like ours but with entries missing `path`/`name`/`id`: still not
    // something we can merge.
    serde_json::from_value(value).map_err(|_| ParseError::NotCommandsFile)
}

/// Work out which of `incoming`'s commands are new to `current`, and what is
/// worth flagging about each.
///
/// `exists` answers whether a resolved path is reachable here; it is a
/// parameter so the rules stay testable without touching a filesystem (the
/// same arrangement [`crate::portable::scan`] uses).
pub fn plan(
    current: &CommandsFile,
    incoming: &CommandsFile,
    vars: &Vars,
    exists: &dyn Fn(&str) -> bool,
) -> Plan {
    let mut seen_ids: HashSet<&str> = current.commands.iter().map(|c| c.id.as_str()).collect();
    let mut seen: HashSet<Identity> = current.commands.iter().map(identity).collect();

    let mut plan = Plan::default();
    for command in &incoming.commands {
        // A shared id means the same command edited elsewhere, not a new one:
        // offering it would be offering a replacement, which this flow does
        // not do.
        if !seen_ids.insert(command.id.as_str()) || !seen.insert(identity(command)) {
            plan.already_present += 1;
            continue;
        }
        let warnings = warnings_for(command, current, &plan.candidates, vars, exists);
        plan.candidates.push(Candidate { command: command.clone(), warnings });
    }
    plan
}

/// Append `selected` to `file`. Every command already there is left exactly as
/// it was — merging adds, and only adds.
///
/// Three things are adjusted on the imported copies on their way in:
///
/// - an id already in use is replaced with a fresh one, so the two commands
///   stay separately editable and deletable;
/// - a shortcut already taken is dropped, because only the first command
///   matching a shortcut ever runs: keeping it would add a dead shortcut and a
///   startup conflict warning rather than a usable command;
/// - the run count starts at zero, because usage is this install's own history
///   and the `total_runs` here never counted those runs.
///
/// Returns how many commands were added.
pub fn merge(file: &mut CommandsFile, selected: &[Command]) -> usize {
    let mut added = 0;
    for command in selected {
        let mut command = command.clone();
        if file.commands.iter().any(|existing| existing.id == command.id) {
            command.id = uuid::Uuid::new_v4().to_string();
        }
        let shortcut = command.shortcut().trim().to_lowercase();
        let taken = !shortcut.is_empty() && file.commands.iter().any(|existing| {
            existing.shortcut().trim().to_lowercase() == shortcut
        });
        if taken {
            command.shortcut = Some(String::new());
        } else if command.shortcut.is_some() {
            // Stored lowercase, the same way the Add dialog stores it.
            command.shortcut = Some(shortcut);
        }
        command.run_count = Some(0);
        file.commands.push(command);
        added += 1;
    }
    added
}

/// What makes two entries the same command: where it points, what it passes,
/// and what it is called.
type Identity = (String, String, String);

fn identity(command: &Command) -> Identity {
    (normalize(&command.path), normalize(command.args()), normalize(&command.name))
}

/// Case and separator spelling both vary between machines for what is the same
/// command — real files hold `c:\Users\Nitropc` and `c:/users/nitropc` side by
/// side — so neither is allowed to make an entry look new. Two genuinely
/// different commands agreeing on path, arguments *and* display name but for
/// letter case do not happen.
fn normalize(value: &str) -> String {
    value.trim().to_lowercase().replace('\\', "/")
}

fn warnings_for(
    command: &Command,
    current: &CommandsFile,
    accepted: &[Candidate],
    vars: &Vars,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<Warning> {
    let mut warnings = Vec::new();

    let shortcut = command.shortcut().trim().to_lowercase();
    if !shortcut.is_empty() {
        let owns = |candidate: &&Command| candidate.shortcut().trim().to_lowercase() == shortcut;
        if let Some(owner) = current.commands.iter().find(|c| owns(c)) {
            warnings.push(Warning::ShortcutTaken {
                shortcut: shortcut.clone(),
                owner: owner.name.clone(),
            });
        } else if let Some(owner) = accepted.iter().find(|c| owns(&&c.command)) {
            warnings.push(Warning::ShortcutTakenInFile {
                shortcut: shortcut.clone(),
                owner: owner.command.name.clone(),
            });
        }
    }

    // A typo inside {{...}} is launched literally, so it is worth flagging
    // wherever it sits — the arguments carry paths just as often as the path
    // field does.
    let unknown_in_path = portable::unknown_placeholders(&command.path, vars);
    for name in unknown_in_path
        .iter()
        .cloned()
        .chain(portable::unknown_placeholders(command.args(), vars))
    {
        let warning = Warning::UnknownVariable { name };
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }

    // An unresolved placeholder survives expansion verbatim, so the path is
    // certain to be missing too; it has already been reported more usefully.
    if !unknown_in_path.is_empty() {
        return warnings;
    }
    // {{browser}} names a handler rather than a file, so there is nothing on
    // disk to look for, and a relative path or a URL is not ours to check.
    if let Target::Path(resolved) = portable::resolve_target(&command.path, vars) {
        if portable::is_absolute_location(&resolved)
            && !portable::looks_like_url(&resolved)
            && !exists(&resolved)
        {
            warnings.push(Warning::MissingPath { path: resolved });
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portable::{VarValue, Vars};

    fn vars() -> Vars {
        Vars::new(
            [
                ("home".to_string(), VarValue::Path("C:\\Users\\ann".to_string())),
                ("browser".to_string(), VarValue::DefaultOpener),
            ],
            true,
            '\\',
        )
    }

    fn file(json: &str) -> CommandsFile {
        serde_json::from_str(json).unwrap()
    }

    fn command(name: &str, path: &str, id: &str, shortcut: &str) -> Command {
        Command {
            path: path.to_string(),
            name: name.to_string(),
            args: None,
            shortcut: Some(shortcut.to_string()),
            id: id.to_string(),
            run_as_admin: None,
            run_count: None,
            extra: Default::default(),
        }
    }

    /// Everything reachable, so only the deliberate misses in a test show up.
    fn all_exist(_: &str) -> bool {
        true
    }

    fn plan_of(current: &CommandsFile, incoming: &CommandsFile) -> Plan {
        plan(current, incoming, &vars(), &all_exist)
    }

    #[test]
    fn parse_rejects_anything_that_is_not_a_commands_document() {
        assert_eq!(parse("{not json").unwrap_err(), ParseError::NotJson);
        assert_eq!(parse("").unwrap_err(), ParseError::NotJson);
        // settings.json and timers.json are valid JSON living in the same folder.
        assert_eq!(parse(r#"{"enable_sounds": true}"#).unwrap_err(), ParseError::NotCommandsFile);
        assert_eq!(parse("[]").unwrap_err(), ParseError::NotCommandsFile);
        // Shaped like ours, but the entries are missing the required fields.
        assert_eq!(
            parse(r#"{"commands": [{"name": "alpha"}]}"#).unwrap_err(),
            ParseError::NotCommandsFile
        );
    }

    #[test]
    fn parse_accepts_a_legacy_shaped_file() {
        let parsed = parse(
            r#"{"commands": [{"path": "a.exe", "name": "alpha", "id": "1"}], "total_runs": 4}"#,
        )
        .unwrap();
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.total_runs, Some(4));
    }

    #[test]
    fn only_commands_that_are_not_here_yet_are_offered() {
        let current = file(
            r#"{"commands": [
                {"path": "a.exe", "name": "alpha", "id": "1"},
                {"path": "b.exe", "name": "beta", "id": "2"}
            ]}"#,
        );
        let incoming = file(
            r#"{"commands": [
                {"path": "a.exe", "name": "alpha", "id": "1"},
                {"path": "c.exe", "name": "gamma", "id": "3"}
            ]}"#,
        );

        let plan = plan_of(&current, &incoming);
        assert_eq!(plan.already_present, 1);
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].command.name, "gamma");
    }

    #[test]
    fn the_same_command_re_added_elsewhere_is_not_new_despite_a_fresh_id() {
        let current = file(r#"{"commands": [{"path": "C:\\Tools\\A.exe", "name": "Alpha", "id": "1"}]}"#);
        // Same command, typed again on another machine: new uuid, and the
        // casing and separators as that machine happened to spell them.
        let incoming =
            file(r#"{"commands": [{"path": "c:/tools/a.exe", "name": "alpha", "id": "9"}]}"#);

        let plan = plan_of(&current, &incoming);
        assert!(plan.candidates.is_empty());
        assert_eq!(plan.already_present, 1);
    }

    #[test]
    fn a_command_edited_elsewhere_is_not_offered_as_a_replacement() {
        let current = file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "id": "1"}]}"#);
        // Same id, re-pointed on the other machine. Merging never replaces, so
        // this must not turn up as something to import.
        let incoming = file(r#"{"commands": [{"path": "moved.exe", "name": "alpha", "id": "1"}]}"#);

        let plan = plan_of(&current, &incoming);
        assert!(plan.candidates.is_empty());
        assert_eq!(plan.already_present, 1);
    }

    #[test]
    fn a_file_that_lists_the_same_command_twice_offers_it_once() {
        let incoming = file(
            r#"{"commands": [
                {"path": "a.exe", "name": "alpha", "id": "1"},
                {"path": "a.exe", "name": "alpha", "id": "2"}
            ]}"#,
        );

        let plan = plan_of(&CommandsFile::default(), &incoming);
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.already_present, 1);
    }

    #[test]
    fn shortcut_clashes_are_reported_against_this_list_and_within_the_file() {
        let current = file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "shortcut": "g", "id": "1"}]}"#);
        let incoming = file(
            r#"{"commands": [
                {"path": "b.exe", "name": "beta", "shortcut": "G", "id": "2"},
                {"path": "c.exe", "name": "gamma", "shortcut": "h", "id": "3"},
                {"path": "d.exe", "name": "delta", "shortcut": "h", "id": "4"},
                {"path": "e.exe", "name": "epsilon", "id": "5"}
            ]}"#,
        );

        let plan = plan_of(&current, &incoming);
        assert_eq!(
            plan.candidates[0].warnings,
            vec![Warning::ShortcutTaken { shortcut: "g".into(), owner: "alpha".into() }]
        );
        assert!(plan.candidates[1].warnings.is_empty(), "the first to claim h keeps it");
        assert_eq!(
            plan.candidates[2].warnings,
            vec![Warning::ShortcutTakenInFile { shortcut: "h".into(), owner: "gamma".into() }]
        );
        assert!(plan.candidates[3].warnings.is_empty(), "no shortcut, nothing to clash");
    }

    #[test]
    fn unknown_variables_are_reported_from_the_path_and_the_arguments() {
        let incoming = file(
            r#"{"commands": [
                {"path": "{{hoem}}\\a.exe", "name": "typo", "id": "1"},
                {"path": "{{home}}\\b.exe", "name": "args", "args": "{{nope}}\\x, plain", "id": "2"}
            ]}"#,
        );

        let plan = plan(&CommandsFile::default(), &incoming, &vars(), &all_exist);
        assert_eq!(
            plan.candidates[0].warnings,
            vec![Warning::UnknownVariable { name: "hoem".into() }],
            "an unresolved placeholder is reported once, not also as a missing file"
        );
        assert_eq!(
            plan.candidates[1].warnings,
            vec![Warning::UnknownVariable { name: "nope".into() }]
        );
    }

    #[test]
    fn only_absolute_paths_that_are_really_missing_are_flagged() {
        let incoming = file(
            r#"{"commands": [
                {"path": "C:\\gone\\a.exe", "name": "missing", "id": "1"},
                {"path": "{{home}}\\here.exe", "name": "present", "id": "2"},
                {"path": "notepad.exe", "name": "relative", "id": "3"},
                {"path": "https://example.com", "name": "url", "id": "4"},
                {"path": "{{browser}}", "name": "handler", "args": "https://example.com", "id": "5"}
            ]}"#,
        );

        let exists = |path: &str| path == "C:\\Users\\ann\\here.exe";
        let plan = plan(&CommandsFile::default(), &incoming, &vars(), &exists);
        assert_eq!(
            plan.candidates[0].warnings,
            vec![Warning::MissingPath { path: "C:\\gone\\a.exe".into() }]
        );
        for candidate in &plan.candidates[1..] {
            assert!(
                candidate.warnings.is_empty(),
                "{} should not be flagged",
                candidate.command.name
            );
        }
    }

    #[test]
    fn merging_appends_and_never_touches_what_is_already_there() {
        let mut current = file(
            r#"{"commands": [
                {"path": "a.exe", "name": "alpha", "shortcut": "g", "id": "1", "run_count": 7}
            ], "total_runs": 7}"#,
        );
        let before = current.commands[0].clone();

        let added = merge(&mut current, &[command("gamma", "c.exe", "3", "z")]);

        assert_eq!(added, 1);
        assert_eq!(current.commands[0], before, "the existing command is untouched");
        assert_eq!(current.commands.len(), 2);
        assert_eq!(current.total_runs, Some(7), "importing is not a run");
    }

    #[test]
    fn an_imported_command_keeping_a_taken_shortcut_would_be_dead_so_it_loses_it() {
        let mut current =
            file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "shortcut": "g", "id": "1"}]}"#);

        merge(&mut current, &[command("beta", "b.exe", "2", "G"), command("gamma", "c.exe", "3", "H")]);

        assert_eq!(current.commands[1].shortcut(), "");
        assert_eq!(current.commands[2].shortcut(), "h", "a free shortcut survives, lowercased");
        assert!(current.shortcut_conflicts().is_empty(), "a merge never creates a conflict");
    }

    #[test]
    fn a_colliding_id_is_replaced_so_both_commands_stay_addressable() {
        let mut current = file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "id": "1"}]}"#);

        // Same id, different command: it can only have got here by the user
        // ticking it, so it is wanted — but it needs an id of its own.
        merge(&mut current, &[command("beta", "b.exe", "1", "")]);

        assert_ne!(current.commands[1].id, "1");
        assert_eq!(current.commands[0].id, "1", "the original keeps its id");
    }

    #[test]
    fn an_imported_run_count_does_not_become_this_installs_history() {
        let mut current = CommandsFile::default();
        let mut busy = command("beta", "b.exe", "2", "");
        busy.run_count = Some(400);

        merge(&mut current, &[busy]);

        assert_eq!(current.commands[0].run_count(), 0);
    }

    #[test]
    fn merging_the_same_file_twice_adds_nothing_the_second_time() {
        let incoming = file(
            r#"{"commands": [
                {"path": "b.exe", "name": "beta", "shortcut": "b", "id": "2"},
                {"path": "c.exe", "name": "gamma", "id": "3"}
            ]}"#,
        );
        let mut current = file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "id": "1"}]}"#);

        let first = plan_of(&current, &incoming);
        let picked: Vec<Command> = first.candidates.iter().map(|c| c.command.clone()).collect();
        assert_eq!(merge(&mut current, &picked), 2);

        let second = plan_of(&current, &incoming);
        assert!(second.candidates.is_empty(), "nothing new is left to offer");
        assert_eq!(second.already_present, 2);
    }

    #[test]
    fn a_command_that_lost_its_shortcut_on_import_still_counts_as_present() {
        // The imported copy is stored without the clashing shortcut, so its
        // identity has to be the part that does not change — otherwise the same
        // file would offer it again on every merge.
        let mut current =
            file(r#"{"commands": [{"path": "a.exe", "name": "alpha", "shortcut": "g", "id": "1"}]}"#);
        let incoming =
            file(r#"{"commands": [{"path": "b.exe", "name": "beta", "shortcut": "g", "id": "2"}]}"#);

        let picked: Vec<Command> =
            plan_of(&current, &incoming).candidates.into_iter().map(|c| c.command).collect();
        merge(&mut current, &picked);

        assert!(plan_of(&current, &incoming).candidates.is_empty());
    }
}
