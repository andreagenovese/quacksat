//! Strip what a TTS voice should not read aloud: reasoning blocks,
//! markdown, emoji. Rust port of the bridge's `speakable()` — hand-rolled
//! (no regex dependency) since the patterns are simple.

pub fn speakable(text: &str) -> String {
    let text = strip_between(text, "<think>", "</think>");
    let text = strip_between(&text, "```", "```");
    let text: String = text
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("• "))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' | '#' => {}
            '[' => {
                // Markdown link: keep the label, drop the (url).
                let label: String = chars.by_ref().take_while(|&c| c != ']').collect();
                out.push_str(&label);
                if chars.peek() == Some(&'(') {
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
            }
            c if is_symbolic(c) => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_between(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        out.push(' ');
        match rest[start + open.len()..].find(close) {
            Some(end) => rest = &rest[start + open.len() + end + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn is_symbolic(c: char) -> bool {
    matches!(u32::from(c),
        0x1F000..=0x1FFFF   // emoji planes
        | 0x2190..=0x21FF   // arrows
        | 0x2300..=0x27BF   // misc technical, dingbats
        | 0x2B00..=0x2BFF
        | 0xFE0F            // variation selector
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markdown_emoji_and_think_blocks() {
        assert_eq!(
            speakable("Fatto! 💡 Ho acceso **Luce Mansarda**."),
            "Fatto! Ho acceso Luce Mansarda."
        );
        assert_eq!(
            speakable("<think>ragiono a lungo</think>Quack! Fatto."),
            "Quack! Fatto."
        );
        assert_eq!(
            speakable("Vedi [la guida](http://x.y) qui → ora"),
            "Vedi la guida qui ora"
        );
        assert_eq!(
            speakable("- elenco `codice` ~~no~~ #titolo"),
            "elenco codice no titolo"
        );
    }
}
