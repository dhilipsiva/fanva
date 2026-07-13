//! The English→Lojban system prompt.
//!
//! Grounded in the gate **by construction**: the grammar section embeds
//! [`gerna::GRAMMAR_EBNF`] verbatim — the exact fragment `gerna::parse_checked` accepts —
//! so the prompt's grammar description can't drift from the parser. A short Notes block
//! adds the place-structure/orthography facts an EBNF can't carry, and the few-shot
//! examples are gate-checked (grammar **and** vocabulary) by `shipped_examples_are_gate_valid`.

use std::sync::LazyLock;

/// English→Lojban. Assembled once at first use from the shipped grammar
/// (`gerna::GRAMMAR_EBNF`) plus hand-written rules, notes, and few-shots. `pub` so docs
/// and tests can inspect it; callers use [`system_prompt`].
pub static LOJBAN_SYSTEM_PROMPT: LazyLock<String> = LazyLock::new(build_prompt);

fn build_prompt() -> String {
    format!(
        r#"You are a Lojban translator. Translate the user's English text into grammatically correct Lojban.

Rules:
- Output ONLY the Lojban translation, nothing else. No explanations, no notes.
- Structure is [sumti] [selbri] [sumti]; put "cu" before the selbri when a leading sumti would otherwise merge into it: "la .adam. cu gerku".
- Gadri: "lo" for veridical descriptions, "le" for non-veridical.
- Names are cmevla wrapped in dots after "la": "Adam" → "la .adam.".
- Tense goes before the selbri: "pu" (past), "ca" (present), "ba" (future).

The parser accepts exactly this grammar (EBNF; terminals are cmavo, written bare):

{ebnf}

Notes (things the grammar above can't show):
- A cmevla MUST end in a consonant. If an English name ends in a vowel, add one: "Mary" → "la .meris." (not "la .maria.").
- A third-person pronoun with no named antecedent is "ko'a" (he/she/it/they).
- A predicate fills its places in order with no preposition for later places. "dunda" is "x1 gives x2 to x3", so the recipient is simply the third sumti: "mi dunda lo cukta la .adam." = "I give the book to Adam".

This is an iterative process. You may receive a follow-up message reporting a grammar or semantic error from a Lojban compiler about your previous output. When you do, correct that output and reply with ONLY the corrected Lojban — no explanation, no apology. Prefer the simplest wording that a strict parser accepts.

Examples:
- "The dog goes to the market" → "lo gerku cu klama lo zarci"
- "I love you" → "mi prami do"
- "Adam sees the cat" → "la .adam. viska lo mlatu"
- "The big dog runs" → "lo barda gerku cu bajra"
- "I ate the food" → "mi pu citka lo cidja"
- "Every dog is an animal" → "ro lo gerku cu danlu"
- "Adam does not eat" → "la .adam. na citka"
- "Adam and the cat eat" → "la .adam. .e lo mlatu cu citka"
- "The cat is seen by Adam" → "lo mlatu cu se viska la .adam."
- "I give the book to Adam" → "mi dunda lo cukta la .adam."
- "She sees the dog" → "ko'a viska lo gerku"
- "Mary sees Adam" → "la .meris. cu viska la .adam.""#,
        ebnf = gerna::GRAMMAR_EBNF
    )
}

/// The system prompt the agent loop passes to `chat()`.
pub fn system_prompt() -> &'static str {
    LOJBAN_SYSTEM_PROMPT.as_str()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::system_prompt;

    /// The grammar block is gerna's own EBNF, embedded verbatim — so the prompt's grammar
    /// cannot drift from what the parser accepts. This is the whole point of the const.
    #[test]
    fn grammar_block_is_gernas_ebnf() {
        assert!(
            system_prompt().contains(gerna::GRAMMAR_EBNF),
            "the prompt must embed gerna::GRAMMAR_EBNF verbatim"
        );
    }

    /// Every few-shot example must (a) pass our own gates and (b) use only vocabulary the
    /// dictionary actually knows. (b) is not implied by (a): smuni defaults an unknown
    /// word to arity 2, so a gate-parse alone would let an out-of-dictionary predicate
    /// through. Run under the curated fallback dictionary (CI) this keeps the prompt from
    /// teaching words the minimal build can't compile; run under the full dictionary it
    /// still holds.
    #[test]
    fn shipped_examples_are_gate_valid() {
        let examples = system_prompt()
            .split("Examples:")
            .nth(1)
            .expect("the prompt has an Examples section");

        let mut checked = 0;
        let mut missing: Vec<String> = Vec::new();
        for line in examples.lines() {
            let Some((_, rhs)) = line.split_once('→') else {
                continue;
            };
            let text = rhs.trim().trim_matches('"');
            if text.is_empty() {
                continue;
            }
            // (a) grammar + semantics gate.
            assert!(
                crate::gates::validate(text).is_ok(),
                "shipped few-shot example is not gate-valid: {text:?} — {:?}",
                crate::gates::validate(text).err()
            );
            // (b) every content word is a real dictionary entry.
            for word in text.split_whitespace() {
                if is_cmevla(word) {
                    continue; // names are not dictionary words
                }
                let key = word.trim_start_matches('.'); // ".e"/".i" connectives → "e"/"i"
                if smuni_dictionary::get_gloss(key).is_none()
                    && smuni_dictionary::get_arity(key).is_none()
                {
                    missing.push(format!("{word:?} (in {text:?})"));
                }
            }
            checked += 1;
        }

        assert!(
            missing.is_empty(),
            "prompt words absent from the dictionary (add them, or use supported words):\n{}",
            missing.join("\n")
        );
        assert!(
            checked >= 5,
            "expected to check the few-shot examples, got {checked}"
        );
    }

    /// A cmevla (name) is dot-delimited on both sides, e.g. ".adam.".
    fn is_cmevla(word: &str) -> bool {
        word.len() > 2 && word.starts_with('.') && word.ends_with('.')
    }
}
