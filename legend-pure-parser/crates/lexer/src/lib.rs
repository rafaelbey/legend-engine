use legend_pure_parser_ast::SourceInfo;
use logos::{Lexer, Logos};
use smol_str::SmolStr;

// We need custom callbacks for islands to consume until `}#`
fn lex_island_content<'a>(lex: &mut Lexer<'a, Token>) -> Option<String> {
    let remainder = lex.remainder();
    if let Some(pos) = remainder.find("}#") {
        lex.bump(pos);
        // lex.slice() includes the `#>` that triggered this
        Some(lex.slice()[2..].to_string())
    } else {
        // Unclosed island
        None
    }
}

/// The core tokens of the Pure Language.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\n\f]+")] // Skip whitespace
#[logos(skip r"//.*")] // Skip single-line comments
#[logos(skip r"/\*([^*]|\*[^/])*\*/")] // Skip multi-line comments
pub enum Token {
    // ─── Brackets & Punctuation ───
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,

    #[token(":")]
    Colon,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("::")]
    PathSep,
    #[token("|")]
    Pipe,
    #[token("->")]
    Arrow,

    // ─── Operators ───
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("=")]
    Eq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token("<=")]
    Lte,
    #[token(">")]
    Gt,
    #[token(">=")]
    Gte,
    #[token("<<")]
    LDoubleAngle,
    #[token(">>")]
    RDoubleAngle,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("!")]
    Bang,

    // ─── Keywords ───
    #[token("Class")]
    KeywordClass,
    #[token("Enum")]
    KeywordEnum,
    #[token("Profile")]
    KeywordProfile,
    #[token("Association")]
    KeywordAssociation,
    #[token("extends")]
    KeywordExtends,
    #[token("function")]
    KeywordFunction,
    #[token("let")]
    KeywordLet,
    #[token("true", |_| true)]
    #[token("false", |_| false)]
    Bool(bool),

    // ─── Identifiers & Literals ───
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*", |lex| SmolStr::new(lex.slice()))]
    Ident(SmolStr),

    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*", |lex| SmolStr::new(&lex.slice()[1..]))]
    Variable(SmolStr),

    #[regex(r"'([^'\\]|\\.)*'", |lex| lex.slice()[1..lex.slice().len()-1].to_string())]
    StringLit(String),

    #[regex(r"-?[0-9]+", |lex| lex.slice().parse().ok())]
    Integer(i64),

    #[regex(r"-?[0-9]+\.[0-9]+([eE][-+]?[0-9]+)?", |lex| lex.slice().parse().ok())]
    Float(f64),

    // Decimal literal: e.g. 1.25d
    #[regex(r"-?[0-9]+(\.[0-9]+)?d", |lex| lex.slice()[..lex.slice().len()-1].to_string())]
    Decimal(String),

    // Date literal: e.g. %2024-03-21T12:00:00
    #[regex(r"%[0-9]{4}-[0-9]{2}-[0-9]{2}(T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?([+-][0-9]{4}|Z)?)?", |lex| lex.slice()[1..].to_string())]
    Date(String),

    // ─── Top-Level Sections & Islands ───
    /// E.g. `###Pure`, `###Connection`, `###Mapping`
    #[regex(r"###[a-zA-Z_0-9]+", |lex| SmolStr::new(&lex.slice()[3..]))]
    SectionHeader(SmolStr),

    /// Standard Island `#>`: `#>content}#`
    #[token("#>", lex_island_content)]
    IslandOpen(String),

    /// Extended Island `#s` or `#q`: `#s{content}#`
    #[regex(r"#[a-zA-Z]+\{", |lex| SmolStr::new(&lex.slice()[1..lex.slice().len()-1]))]
    IslandExtendedOpen(SmolStr),

    /// The closing token for extended islands `}#`
    #[token("}#")]
    IslandClose,
}

impl Token {
    pub fn as_ident(&self) -> Option<String> {
        match self {
            Token::Ident(id) => Some(id.to_string()),
            Token::KeywordClass => Some("Class".to_string()),
            Token::KeywordEnum => Some("Enum".to_string()),
            Token::KeywordProfile => Some("Profile".to_string()),
            Token::KeywordAssociation => Some("Association".to_string()),
            Token::KeywordExtends => Some("extends".to_string()),
            Token::KeywordFunction => Some("function".to_string()),
            Token::KeywordLet => Some("let".to_string()),
            _ => None,
        }
    }
}

pub struct SpannedToken {
    pub token: Token,
    pub source_info: SourceInfo,
}

pub struct LineIndexer {
    source_id: String,
    line_starts: Vec<usize>,
}

impl LineIndexer {
    pub fn new(source_id: impl Into<String>, source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, c) in source.char_indices() {
            if c == '\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            source_id: source_id.into(),
            line_starts,
        }
    }

    pub fn byte_to_line_col(&self, byte_index: usize) -> (usize, usize) {
        // Binary search for the line
        let line = match self.line_starts.binary_search(&byte_index) {
            Ok(idx) => idx,
            Err(idx) => idx - 1,
        };
        let col = byte_index - self.line_starts[line];
        (line + 1, col + 1) // 1-indexed for IDEs
    }

    pub fn span_to_source_info(&self, span: std::ops::Range<usize>) -> SourceInfo {
        let (start_line, start_column) = self.byte_to_line_col(span.start);
        let (end_line, end_column) = self.byte_to_line_col(span.end);
        
        SourceInfo::new(
            self.source_id.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
        )
    }
}

pub fn lex_source(source_id: &str, source: &str) -> Vec<SpannedToken> {
    let indexer = LineIndexer::new(source_id, source);
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    
    while let Some(res) = lexer.next() {
        if let Ok(token) = res {
            tokens.push(SpannedToken {
                token,
                source_info: indexer.span_to_source_info(lexer.span()),
            });
        }
    }
    
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_def() {
        let code = "
        Class model::Person {
            name: String[1];
        }
        ";
        let tokens = lex_source("test.pure", code);
        let kinds: Vec<Token> = tokens.into_iter().map(|t| t.token).collect();

        assert_eq!(
            kinds,
            vec![
                Token::KeywordClass,
                Token::Ident("model".into()),
                Token::PathSep,
                Token::Ident("Person".into()),
                Token::LBrace,
                Token::Ident("name".into()),
                Token::Colon,
                Token::Ident("String".into()),
                Token::LBracket,
                Token::Integer(1),
                Token::RBracket,
                Token::Semi,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn test_island_grammar() {
        let code = r#"#>db.schema.table}#"#;
        let tokens = lex_source("test.pure", code);
        assert_eq!(tokens.len(), 2); // IslandOpen + IslandClose
        match &tokens[0].token {
            Token::IslandOpen(content) => assert_eq!(content, "db.schema.table"),
            _ => panic!("Expected IslandOpen"),
        }
    }

    #[test]
    fn test_line_indexer() {
        let code = "Class A\n{\n  name: String[1];\n}";
        let tokens = lex_source("test.pure", code);
        
        let class_token = &tokens[0]; // 'Class'
        assert_eq!(class_token.source_info.start_line, 1);
        assert_eq!(class_token.source_info.start_column, 1);
        assert_eq!(class_token.source_info.end_line, 1);
        assert_eq!(class_token.source_info.end_column, 6);

        let name_token = &tokens[3]; // 'name'
        assert_eq!(name_token.source_info.start_line, 3);
        assert_eq!(name_token.source_info.start_column, 3);
        assert_eq!(name_token.source_info.end_line, 3);
        assert_eq!(name_token.source_info.end_column, 7);
    }
}
