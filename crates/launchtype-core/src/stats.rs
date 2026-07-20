//! Stats mode ("!"): the read-only command usage summary, ported from
//! `DataManager.get_stats_items`.

use crate::i18n::{format_args, tr, Arg};
use crate::model::CommandsFile;

/// The stats-mode list labels, in display order: lifetime total, 10 most used
/// (only commands actually run; otherwise a "nothing run yet" line), 10 least
/// used (zero-run commands included).
pub fn stats_labels(file: &CommandsFile) -> Vec<String> {
    let commands = &file.commands;
    let total = file.total_runs.unwrap_or(0);

    let mut labels = vec![format_args(
        &tr("Total commands run: {count}"),
        &[("count", Arg::Int(total as i64))],
    )];

    // Stable sorts keep insertion order for equal counts, like Python's sorted.
    let mut ranked: Vec<&crate::model::Command> = commands.iter().collect();
    ranked.sort_by(|a, b| b.run_count().cmp(&a.run_count()));
    let most_used: Vec<_> = ranked.iter().filter(|c| c.run_count() > 0).take(10).collect();
    if most_used.is_empty() {
        labels.push(tr("No commands have been run yet."));
    } else {
        for (rank, command) in most_used.iter().enumerate() {
            labels.push(format_args(
                &tr("Most used {rank}: {name}, {count} runs"),
                &[
                    ("rank", Arg::Int(rank as i64 + 1)),
                    ("name", Arg::Str(&command.name)),
                    ("count", Arg::Int(command.run_count() as i64)),
                ],
            ));
        }
    }

    let mut ascending: Vec<&crate::model::Command> = commands.iter().collect();
    ascending.sort_by_key(|c| c.run_count());
    for (rank, command) in ascending.iter().take(10).enumerate() {
        labels.push(format_args(
            &tr("Least used {rank}: {name}, {count} runs"),
            &[
                ("rank", Arg::Int(rank as i64 + 1)),
                ("name", Arg::Str(&command.name)),
                ("count", Arg::Int(command.run_count() as i64)),
            ],
        ));
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with_counts(counts: &[(&str, u64)], total: u64) -> CommandsFile {
        let commands = counts
            .iter()
            .map(|(name, count)| {
                serde_json::from_value(serde_json::json!({
                    "path": "x.exe",
                    "name": name,
                    "id": name,
                    "run_count": count,
                }))
                .unwrap()
            })
            .collect();
        CommandsFile { commands, total_runs: Some(total), extra: Default::default() }
    }

    #[test]
    fn ranks_most_and_least_used() {
        let file = file_with_counts(&[("alpha", 5), ("beta", 0), ("gamma", 9)], 14);
        let labels = stats_labels(&file);
        assert_eq!(labels[0], "Total commands run: 14");
        assert_eq!(labels[1], "Most used 1: gamma, 9 runs");
        assert_eq!(labels[2], "Most used 2: alpha, 5 runs");
        // beta (0 runs) excluded from most-used, included in least-used.
        assert_eq!(labels[3], "Least used 1: beta, 0 runs");
        assert_eq!(labels[4], "Least used 2: alpha, 5 runs");
        assert_eq!(labels[5], "Least used 3: gamma, 9 runs");
        assert_eq!(labels.len(), 6);
    }

    #[test]
    fn nothing_run_yet_message() {
        let file = file_with_counts(&[("alpha", 0)], 0);
        let labels = stats_labels(&file);
        assert_eq!(labels[1], "No commands have been run yet.");
        assert_eq!(labels[2], "Least used 1: alpha, 0 runs");
    }

    #[test]
    fn equal_counts_keep_insertion_order() {
        let file = file_with_counts(&[("first", 3), ("second", 3), ("third", 3)], 9);
        let labels = stats_labels(&file);
        assert_eq!(labels[1], "Most used 1: first, 3 runs");
        assert_eq!(labels[2], "Most used 2: second, 3 runs");
        assert_eq!(labels[3], "Most used 3: third, 3 runs");
    }
}
