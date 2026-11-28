use mago_database::file::FileId;
use mago_syntax::lexer::Lexer;
use mago_syntax::settings::LexerSettings;
use mago_syntax::token::TokenKind;
use mago_syntax_core::input::Input;

const SEPARATOR: u8 = 0;
const COMMA: [u8; 2] = [b',', SEPARATOR];

fn collapsed(text: &[u8], into: &mut Vec<u8>) {
    for word in text
        .split(u8::is_ascii_whitespace)
        .filter(|word| return !word.is_empty())
    {
        into.extend_from_slice(word);
        into.push(b' ');
    }
    into.push(SEPARATOR);
}

pub(crate) fn normalized<'d>(docblocks: impl Iterator<Item = &'d [u8]>, code: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    for docblock in docblocks {
        collapsed(docblock, &mut text);
    }
    let mut lexer = Lexer::scripting(Input::new(FileId::zero(), code), LexerSettings::default());
    let start = text.len();
    while let Some(token) = lexer.advance() {
        let Ok(token) = token else {
            text.truncate(start);
            text.extend_from_slice(code);
            return text;
        };
        match token.kind {
            TokenKind::Whitespace
            | TokenKind::SingleLineComment
            | TokenKind::HashComment
            | TokenKind::MultiLineComment => {}
            TokenKind::DocBlockComment => collapsed(token.value, &mut text),
            TokenKind::RightParenthesis | TokenKind::RightBracket => {
                if text.ends_with(&COMMA)
                    && text.len().checked_sub(COMMA.len()).is_some_and(|before| {
                        return before == 0
                            || text.get(before.saturating_sub(1)) == Some(&SEPARATOR);
                    })
                {
                    text.truncate(text.len().saturating_sub(COMMA.len()));
                }
                text.extend_from_slice(token.value);
                text.push(SEPARATOR);
            }
            _ => {
                text.extend_from_slice(token.value);
                text.push(SEPARATOR);
            }
        }
    }
    return text;
}
