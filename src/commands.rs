use rand::seq::SliceRandom;
use crate::events::Command;

const SESSION_ADJECTIVES: &[&str] = &[
    "amber", "arctic", "ashen", "azure", "basalt", "blazing", "boreal", "brazen",
    "brisk", "bronze", "cedar", "cobalt", "copper", "coral", "crimson", "crypt",
    "dusk", "ember", "feral", "ferric", "flint", "fossil", "frozen", "gilded",
    "glacial", "granite", "hollow", "hushed", "iron", "ivory", "jagged", "lunar",
    "molten", "moss", "mystic", "neon", "nimble", "obsidian", "onyx", "opaque",
    "pale", "phantom", "plume", "quartz", "riven", "runic", "rustic", "sable",
    "scarlet", "silver", "slate", "smoked", "solar", "stark", "tawny", "umbral",
    "velvet", "vivid", "woven", "zinc",
];

const SESSION_NOUNS: &[&str] = &[
    "anchor", "anvil", "badger", "bastion", "beacon", "bison", "cairn", "chalice",
    "cipher", "compass", "condor", "coyote", "dagger", "drake", "falcon", "forge",
    "frigate", "garnet", "griffin", "harbor", "herald", "hornet", "jackal", "javelin",
    "lantern", "locus", "mammoth", "mantis", "marlin", "monolith", "nexus", "obelisk",
    "osprey", "outpost", "panther", "pebble", "pilgrim", "plinth", "prism", "pylon",
    "quarry", "raven", "ridgeback", "scepter", "schooner", "sentinel", "serpent",
    "sigil", "sparrow", "spindle", "summit", "talon", "tempest", "thistle", "trident",
    "tundra", "vanguard", "vortex", "warden", "zenith",
];

pub fn generate_session_name(existing: &[String]) -> String {
    let mut rng = rand::thread_rng();
    for _ in 0..100 {
        let adj = SESSION_ADJECTIVES.choose(&mut rng).unwrap();
        let noun = SESSION_NOUNS.choose(&mut rng).unwrap();
        let name = format!("{} {}", adj, noun);
        if !existing.contains(&name) {
            return name;
        }
    }
    let adj = SESSION_ADJECTIVES.choose(&mut rng).unwrap();
    let noun = SESSION_NOUNS.choose(&mut rng).unwrap();
    format!("{} {} {}", adj, noun, existing.len() + 1)
}

fn command_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.to_lowercase())
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut matrix = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() { matrix[i][0] = i; }
    for j in 0..=b.len() { matrix[0][j] = j; }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i-1] == b[j-1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i-1][j] + 1)
                .min(matrix[i][j-1] + 1)
                .min(matrix[i-1][j-1] + cost);
        }
    }
    matrix[a.len()][b.len()]
}

pub fn fuzzy_match_session(query: &str, session_names: &[String]) -> Option<String> {
    let query_lower = query.to_lowercase();
    let threshold = 0.35;

    session_names.iter()
        .filter_map(|name| {
            let name_lower = name.to_lowercase();
            let dist = levenshtein(&query_lower, &name_lower);
            let max_len = query_lower.len().max(name_lower.len());
            if max_len == 0 { return None; }
            let normalized = dist as f64 / max_len as f64;
            if normalized <= threshold {
                Some((name.clone(), normalized))
            } else {
                None
            }
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(name, _)| name)
}

fn strip_punctuation(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>()
        .split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_command_with_sessions(transcript: &str, session_names: &[String]) -> Option<Command> {
    // First try standard command parsing
    if let Some(cmd) = parse_command(transcript) {
        return Some(cmd);
    }

    // Then try bare session name fuzzy match (e.g. just saying "arctic pebble")
    let text = strip_punctuation(&transcript.trim().to_lowercase());
    if !text.is_empty() {
        if let Some(matched) = fuzzy_match_session(&text, session_names) {
            return Some(Command::SwitchSession(matched));
        }
    }

    None
}

pub fn parse_command(transcript: &str) -> Option<Command> {
    let words = command_words(transcript);
    let text = words.join(" ");

    // Session commands
    if text.contains("new session") || text.contains("new conversation")
        || text.contains("reset session") || text.contains("clear context")
        || text.contains("start over") || text.contains("fresh start") {
        return Some(Command::NewSession);
    }

    if text.contains("list sessions") || text.contains("show sessions") {
        return Some(Command::ListSessions);
    }

    if text.contains("delete session") || text.contains("remove session") {
        return Some(Command::DeleteSession);
    }

    // Switch session (extract query after keyword)
    for prefix in &["switch session", "go to session", "switch to"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let query = rest.trim().to_string();
            if !query.is_empty() {
                return Some(Command::SwitchSession(query));
            }
        }
    }

    // Rename session
    for prefix in &["rename session", "name session"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let name = rest.trim().to_string();
            if !name.is_empty() {
                return Some(Command::RenameSession(name));
            }
        }
    }

    // Voice toggle
    if text.contains("voice on") || text.contains("speech on") {
        return Some(Command::VoiceOn);
    }
    if text.contains("voice off") || text.contains("speech off") {
        return Some(Command::VoiceOff);
    }

    // Help
    if text == "help" || text == "commands" || text.contains("show help") || text.contains("show commands") {
        return Some(Command::Help);
    }

    // Confirmation (for delete)
    if matches!(text.as_str(), "yes" | "yeah" | "yep" | "confirm" | "do it") {
        return Some(Command::ConfirmDelete);
    }
    if matches!(text.as_str(), "no" | "nope" | "cancel" | "never mind") {
        return Some(Command::CancelDelete);
    }

    None
}

pub fn is_cancel_keyword(text: &str) -> bool {
    let lower = strip_punctuation(&text.to_lowercase());
    lower.contains("ignore this")
        || lower.contains("never mind")
        || lower.contains("forget it")
        || lower.contains("scratch that")
}

pub fn truncate_to_sentences(text: &str, max_sentences: usize) -> String {
    let mut count = 0;
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' {
            count += 1;
            end = i + c.len_utf8();
            if count >= max_sentences {
                return text[..end].trim().to_string();
            }
        }
    }
    text.trim().to_string()
}
