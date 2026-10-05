//! Normalisation de texte pour les correspondances par mots-clés (règles et simulateurs démo).

/// Minuscules, accents retirés, ponctuation remplacée par des espaces,
/// espaces simples, et un espace en tête et en fin pour des correspondances « mot entier ».
pub fn normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push(' ');
    let mut last_space = true;
    for ch in input.chars().flat_map(char::to_lowercase) {
        let mapped: &str = match ch {
            'à' | 'â' | 'ä' | 'á' | 'ã' => "a",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'î' | 'ï' | 'í' => "i",
            'ô' | 'ö' | 'ó' => "o",
            'ù' | 'û' | 'ü' | 'ú' => "u",
            'ç' => "c",
            'œ' => "oe",
            'æ' => "ae",
            'ñ' => "n",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                last_space = false;
                continue;
            }
            _ => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
                continue;
            }
        };
        out.push_str(mapped);
        last_space = false;
    }
    if !last_space {
        out.push(' ');
    }
    out
}

/// Vrai si `phrase` (déjà normalisée ou non) apparaît en mots entiers dans `haystack` normalisé.
pub fn contains_phrase(haystack_normalized: &str, phrase: &str) -> bool {
    let needle = normalize(phrase);
    !needle.trim().is_empty() && haystack_normalized.contains(&needle)
}

/// Estimation grossière du nombre de tokens (≈ 4 caractères par token).
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// Langue probable (« fr » ou « en ») par comptage de mots outils.
pub fn guess_language(text: &str) -> &'static str {
    let norm = normalize(text);
    let count = |words: &[&str]| {
        words
            .iter()
            .filter(|w| norm.contains(&format!(" {w} ")))
            .count()
    };
    let en = count(&[
        "the", "and", "you", "please", "we", "our", "would", "could", "thanks", "hello", "hi",
    ]);
    let fr = count(&[
        "le", "la", "les", "vous", "nous", "merci", "pour", "bonjour", "votre", "une", "est",
    ]);
    if en > fr { "en" } else { "fr" }
}

/// Première phrase (ou début) d'un texte, bornée en caractères.
pub fn first_sentence(text: &str, max_chars: usize) -> String {
    let cleaned: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let n = normalize(l);
            ![
                "bonjour",
                "hello",
                "hi",
                "hi there",
                "salut",
                "madame monsieur",
                "dear hiring team",
            ]
            .iter()
            .any(|g| n.trim() == *g)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let end = cleaned
        .find(['.', '?', '!'])
        .map(|i| i + 1)
        .unwrap_or(cleaned.len());
    let sentence = &cleaned[..end];
    if sentence.chars().count() > max_chars {
        let truncated: String = sentence.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        sentence.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accents_and_punctuation() {
        assert_eq!(normalize("Échéance : 31/10, RIB!"), " echeance 31 10 rib ");
    }

    #[test]
    fn matches_whole_words_only() {
        let n = normalize("Votre contribution annuelle");
        assert!(!contains_phrase(&n, "rib"));
        assert!(contains_phrase(&normalize("Nouveau RIB ci-joint"), "rib"));
    }

    #[test]
    fn guesses_language() {
        assert_eq!(
            guess_language("Hello, could you send the quote please?"),
            "en"
        );
        assert_eq!(
            guess_language("Bonjour, pouvez-vous nous envoyer le devis ?"),
            "fr"
        );
    }
}
