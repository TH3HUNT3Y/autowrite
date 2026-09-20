#[derive(Clone, Debug)]
pub struct WordToken {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub difficult: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TextAnalysis {
    pub characters: usize,
    pub words: usize,
    pub sentences: usize,
    pub paragraphs: usize,
    pub punctuation: usize,
    pub tokens: Vec<WordToken>,
}

impl TextAnalysis {
    pub fn parse(source: &str) -> Self {
        let mut result = Self {
            characters: source.chars().count(),
            paragraphs: if source.is_empty() {
                0
            } else {
                source.split("\n\n").count()
            },
            ..Self::default()
        };
        let mut byte_start = None;
        for (index, character) in source.char_indices() {
            if character.is_alphanumeric() || character == '\'' {
                if byte_start.is_none() {
                    byte_start = Some(index);
                }
            } else if let Some(start) = byte_start.take() {
                result.add_word(source, start, index);
            }
            if matches!(character, '.' | '!' | '?') {
                result.sentences += 1;
            }
            if character.is_ascii_punctuation() {
                result.punctuation += 1;
            }
        }
        if let Some(start) = byte_start {
            result.add_word(source, start, source.len());
        }
        result
    }

    fn add_word(&mut self, source: &str, start: usize, end: usize) {
        let text = source[start..end].to_string();
        self.tokens.push(WordToken {
            start,
            end,
            difficult: text.chars().count() >= 10,
            text,
        });
        self.words += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::TextAnalysis;

    #[test]
    fn preserves_structural_counts() {
        let analysis = TextAnalysis::parse("One, two.\n\nA longer paragraph!");
        assert_eq!(analysis.characters, 30);
        assert_eq!(analysis.words, 5);
        assert_eq!(analysis.sentences, 2);
        assert_eq!(analysis.paragraphs, 2);
        assert_eq!(analysis.punctuation, 3);
    }
}
