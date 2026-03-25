use legend_pure_parser_ast::*;
use legend_pure_parser_lexer::{lex_source, SpannedToken, Token};
use linkme::distributed_slice;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[cfg(test)]
mod document_tests;
pub mod expr;
#[cfg(test)]
mod expr_tests;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Unexpected token {0:?} at position {1}")]
    UnexpectedToken(Token, usize),
    #[error("Unexpected end of input")]
    UnexpectedEof,
    #[error("Unknown section: {0}")]
    UnknownSection(String),
    #[error("Engine error: {0}")]
    EngineError(String, legend_pure_parser_ast::SourceInfo),
}

// ═══════════════════════════════════════════════════
// PLUGIN TRAITS & REGISTRY
// ═══════════════════════════════════════════════════

#[distributed_slice]
pub static ISLAND_PLUGINS: [fn() -> Box<dyn IslandPlugin>];

#[distributed_slice]
pub static SECTION_PLUGINS: [fn() -> Box<dyn SectionPlugin>];

#[distributed_slice]
pub static SUB_PARSERS: [fn() -> Arc<dyn SubParser>];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubParserCategory {
    ConnectionValue,
    MappingElement,
    MappingInclude,
    EmbeddedData,
    TestAssertion,
}

pub trait IslandPlugin: Send + Sync {
    fn island_type(&self) -> &str;
    fn parse(&self, content: &str, source_info: SourceInfo) -> Result<ClassInstance, ParseError>;
}

pub trait SectionPlugin: Send + Sync {
    fn section_name(&self) -> &str;
    fn parse_section(
        &self,
        tokens: &[SpannedToken],
        pos: &mut usize,
        source: &str,
        registry: &PluginRegistry,
    ) -> Result<Vec<ExtensionElement>, ParseError>;
}

pub trait SubParser: Send + Sync {
    fn category(&self) -> SubParserCategory;
    fn sub_type(&self) -> &str;
    fn parse_tokens(
        &self,
        tokens: &[SpannedToken],
        pos: &mut usize,
        source: &str,
        registry: &PluginRegistry,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>, ParseError>;
}

pub struct PluginRegistry {
    pub island_plugins: HashMap<String, Arc<dyn IslandPlugin>>,
    pub section_plugins: HashMap<String, Arc<dyn SectionPlugin>>,
    pub sub_parsers: HashMap<SubParserCategory, HashMap<String, Arc<dyn SubParser>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            island_plugins: HashMap::new(),
            section_plugins: HashMap::new(),
            sub_parsers: HashMap::new(),
        }
    }

    pub fn get_sub_parser(
        &self,
        category: SubParserCategory,
        sub_type: &str,
    ) -> Option<&Arc<dyn SubParser>> {
        self.sub_parsers.get(&category)?.get(sub_type)
    }
}

// ═══════════════════════════════════════════════════
// CORE PARSER
// ═══════════════════════════════════════════════════

pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<SpannedToken>,
    pos: usize,
    registry: Arc<PluginRegistry>,
}

impl<'a> Parser<'a> {
    pub fn new(source_id: &str, source: &'a str, registry: Arc<PluginRegistry>) -> Self {
        Self {
            source,
            tokens: lex_source(source_id, source),
            pos: 0,
            registry,
        }
    }

    pub fn parse_document(&mut self) -> Result<Vec<Element>, ParseError> {
        let mut elements = Vec::new();
        while self.pos < self.tokens.len() {
            let token = &self.tokens[self.pos].token;
            match token {
                Token::SectionHeader(name) => {
                    if name == "Pure" {
                        self.pos += 1; // Consume header
                        elements.extend(self.parse_pure_section()?);
                    } else if let Some(plugin) = self.registry.section_plugins.get(name.as_str()) {
                        self.pos += 1;
                        let mut section_elements = plugin.parse_section(
                            &self.tokens,
                            &mut self.pos,
                            self.source,
                            &self.registry,
                        )?;
                        elements.extend(section_elements.into_iter().map(Element::Extension));
                    } else {
                        return Err(ParseError::UnknownSection(name.to_string()));
                    }
                }
                _ => {
                    // Start of file without explicit header, optionally default to Pure section
                    elements.extend(self.parse_pure_section()?);
                }
            }
        }
        Ok(elements)
    }

    fn parse_pure_section(&mut self) -> Result<Vec<Element>, ParseError> {
        let mut elements = Vec::new();
        while self.pos < self.tokens.len() {
            match self.tokens[self.pos].token {
                Token::KeywordClass => elements.push(Element::Class(self.parse_class()?)),
                Token::KeywordProfile => elements.push(Element::Profile(self.parse_profile()?)),
                Token::KeywordFunction => {
                    self.parse_function()?; /* Stub: ignore result for now */
                }
                Token::SectionHeader(_) => break, // Hit a new section
                _ => {
                    return Err(ParseError::UnexpectedToken(
                        self.tokens[self.pos].token.clone(),
                        self.pos,
                    ))
                }
            }
        }
        Ok(elements)
    }

    fn parse_class(&mut self) -> Result<ClassDef, ParseError> {
        let start_info = self.tokens[self.pos].source_info.clone();
        self.pos += 1; // Consume 'Class'

        let stereotypes = self.parse_optional_stereotypes()?;
        let tagged_values = self.parse_optional_tagged_values()?;

        let mut pkg_path = Vec::new();
        let mut name = SmolStr::default();

        // 1. Parse qualified name (path::to::Class)
        while self.pos < self.tokens.len() {
            if let Some(id) = self.tokens[self.pos].token.as_ident() {
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].token == Token::PathSep
                {
                    pkg_path.push(id.to_string());
                    self.pos += 2; // Consume Ident and ::
                } else {
                    name = id.clone().into();
                    self.pos += 1;
                    break;
                }
            } else {
                return Err(ParseError::UnexpectedToken(
                    self.tokens[self.pos].token.clone(),
                    self.pos,
                ));
            }
        }

        self.check_type_parameters()?;

        let super_types = self.parse_optional_extends()?;

        // 2. Expect '{'
        self.expect_token(Token::LBrace)?;

        let mut properties = Vec::new();

        // 3. Parse properties (stub for properties and functions within class body)
        while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RBrace {
            if self.tokens[self.pos].token == Token::LBrace
                || self.tokens[self.pos].token == Token::LDoubleAngle
                || matches!(self.tokens[self.pos].token, Token::Ident(_))
            {
                // Determine if it's a property or a function by looking ahead for `(` before `:`
                let mut is_func = false;
                for i in self.pos..self.tokens.len() {
                    if self.tokens[i].token == Token::LParen {
                        is_func = true;
                        break;
                    }
                    if self.tokens[i].token == Token::Colon {
                        break;
                    }
                }

                if is_func {
                    self.skip_until(Token::Semi)?; // stub skip of inline functions for now
                } else {
                    properties.push(self.parse_property()?);
                }
            } else {
                self.pos += 1; // skip unknown body tokens in stub
            }
        }

        let end_info = if self.pos < self.tokens.len() {
            self.tokens[self.pos].source_info.clone()
        } else {
            self.tokens.last().unwrap().source_info.clone()
        };
        // 4. Expect '}'
        self.expect_token(Token::RBrace)?;

        let source_info = SourceInfo {
            source_id: start_info.source_id.clone(),
            start_line: start_info.start_line,
            end_line: end_info.end_line,
            start_column: start_info.start_column,
            end_column: end_info.end_column,
        };

        Ok(ClassDef {
            package: PackagePath {
                path: pkg_path,
                source_info: source_info.clone(),
            },
            name,
            super_types,
            properties,
            qualified_properties: Vec::new(),
            constraints: Vec::new(),
            stereotypes,
            tagged_values,
            source_info,
        })
    }

    fn skip_until(&mut self, token: Token) -> Result<(), ParseError> {
        while self.pos < self.tokens.len() {
            if self.tokens[self.pos].token == token {
                self.pos += 1;
                return Ok(());
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn parse_optional_stereotypes(&mut self) -> Result<Vec<StereotypePtr>, ParseError> {
        let mut st = Vec::new();
        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::LDoubleAngle {
            self.pos += 1;
            while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RDoubleAngle
            {
                let prof = match &self.tokens[self.pos].token {
                    Token::Ident(p) => Some(p.to_string()),
                    _ => None,
                };
                if let Some(prof_str) = prof {
                    self.pos += 1;
                    if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Dot {
                        self.pos += 1;
                        let val = match &self.tokens[self.pos].token {
                            Token::Ident(v) => Some(v.to_string()),
                            _ => None,
                        };
                        if let Some(val_str) = val {
                            st.push(StereotypePtr {
                                profile: prof_str,
                                value: SmolStr::new(val_str),
                                profile_source_info: SourceInfo::dummy(),
                                source_info: SourceInfo::dummy(),
                            });
                            self.pos += 1;
                        }
                    }
                }
                if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Comma {
                    self.pos += 1;
                }
            }
            self.expect_token(Token::RDoubleAngle)?;
        }
        Ok(st)
    }

    fn parse_optional_tagged_values(&mut self) -> Result<Vec<TaggedValue>, ParseError> {
        let mut tv = Vec::new();
        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::LBrace {
            let mut is_tv = false;
            if self.pos + 2 < self.tokens.len() {
                if matches!(self.tokens[self.pos + 1].token, Token::Ident(_))
                    && self.tokens[self.pos + 2].token == Token::Dot
                {
                    is_tv = true;
                }
            }
            if is_tv {
                self.pos += 1;
                while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RBrace {
                    let prof = match &self.tokens[self.pos].token {
                        Token::Ident(p) => Some(p.to_string()),
                        _ => None,
                    };
                    if let Some(prof_str) = prof {
                        self.pos += 1;
                        if self.expect_token(Token::Dot).is_ok() {
                            let tag = match &self.tokens[self.pos].token {
                                Token::Ident(t) => Some(t.to_string()),
                                _ => None,
                            };
                            if let Some(tag_str) = tag {
                                self.pos += 1;
                                if self.expect_token(Token::Eq).is_ok() {
                                    let val = match &self.tokens[self.pos].token {
                                        Token::StringLit(v) => Some(v.to_string()),
                                        _ => None,
                                    };
                                    if let Some(val_str) = val {
                                        tv.push(TaggedValue {
                                            profile: prof_str,
                                            profile_source_info: SourceInfo::dummy(),
                                            tag: SmolStr::new(tag_str),
                                            value: val_str,
                                            source_info: SourceInfo::dummy(),
                                        });
                                        self.pos += 1;
                                    }
                                }
                            }
                        }
                    }
                    if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Comma {
                        self.pos += 1;
                    }
                }
                self.expect_token(Token::RBrace)?;
            }
        }
        Ok(tv)
    }

    fn parse_function(&mut self) -> Result<(), ParseError> {
        self.pos += 1; // Consume 'function'

        // consume name up to < or (
        while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::LBrace {
            if self.tokens[self.pos].token == Token::Lt {
                let lt_info = self.tokens[self.pos].source_info.clone();
                let mut temp_pos = self.pos;
                while temp_pos < self.tokens.len() && self.tokens[temp_pos].token != Token::Gt {
                    temp_pos += 1;
                }
                let end_info = if temp_pos < self.tokens.len() {
                    self.tokens[temp_pos].source_info.clone()
                } else {
                    lt_info.clone()
                };
                let src = SourceInfo::new(
                    lt_info.source_id,
                    lt_info.start_line,
                    lt_info.start_column,
                    end_info.end_line,
                    end_info.end_column,
                );
                return Err(ParseError::EngineError(
                    "Type and/or multiplicity parameters are not authorized in Legend Engine"
                        .to_string(),
                    src,
                ));
            }
            self.pos += 1;
        }

        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::LBrace {
            self.pos += 1;
        }

        let mut brace_depth = 1;
        while self.pos < self.tokens.len() && brace_depth > 0 {
            if self.tokens[self.pos].token == Token::LBrace {
                brace_depth += 1;
            } else if self.tokens[self.pos].token == Token::RBrace {
                brace_depth -= 1;
                if brace_depth == 0 {
                    self.pos += 1;
                    break;
                }
            } else if self.tokens[self.pos].token == Token::Arrow {
                // Peek ahead for subType and @
                if self.pos + 2 < self.tokens.len() {
                    if let Some(id) = self.tokens[self.pos + 1].token.as_ident() {
                        if id == "subType" {
                            let src = self.tokens[self.pos].source_info.clone();
                            let mut end_pos = self.pos + 2;
                            // Search for closing parenthesis to capture full error bounds
                            while end_pos < self.tokens.len()
                                && self.tokens[end_pos].token != Token::RParen
                            {
                                end_pos += 1;
                            }

                            let end_info = if end_pos < self.tokens.len() {
                                self.tokens[end_pos].source_info.clone()
                            } else {
                                self.tokens[self.pos + 1].source_info.clone()
                            };

                            return Err(ParseError::EngineError(
                                "->subType() is supported only at root level".to_string(),
                                SourceInfo::new(
                                    src.source_id,
                                    src.start_line,
                                    src.start_column,
                                    end_info.end_line,
                                    end_info.end_column,
                                ),
                            ));
                        }
                    }
                }
            } else if let Token::IslandExtendedOpen(plugin_name) = &self.tokens[self.pos].token {
                if !self
                    .registry
                    .island_plugins
                    .contains_key(plugin_name.as_str())
                {
                    let src = self.tokens[self.pos].source_info.clone();
                    let mut end_pos = self.pos + 1;
                    let mut island_brace_depth = 1;
                    // Scan forward to accurately close the island to get the end coordinates
                    while end_pos < self.tokens.len() && island_brace_depth > 0 {
                        if let Token::IslandExtendedOpen(_) = self.tokens[end_pos].token {
                            island_brace_depth += 1;
                        } else if self.tokens[end_pos].token == Token::IslandClose {
                            island_brace_depth -= 1;
                        }
                        end_pos += 1;
                    }
                    let end_info = if end_pos <= self.tokens.len() {
                        self.tokens[end_pos - 1].source_info.clone()
                    } else {
                        src.clone()
                    };

                    let mut available: Vec<String> =
                        self.registry.island_plugins.keys().cloned().collect();
                    available.sort();
                    let msg = format!(
                        "Can't find an embedded Pure parser for the type '{}' available ones: [{}]",
                        plugin_name,
                        available.join(", ")
                    );
                    return Err(ParseError::EngineError(
                        msg,
                        SourceInfo::new(
                            src.source_id,
                            src.start_line,
                            src.start_column,
                            end_info.end_line,
                            end_info.end_column,
                        ),
                    ));
                }
            }
            self.pos += 1;
        }

        // Sometimes there are double brackets like '{ } { content }' in mapping functions
        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::LBrace {
            brace_depth = 1;
            self.pos += 1;
            while self.pos < self.tokens.len() && brace_depth > 0 {
                if self.tokens[self.pos].token == Token::LBrace {
                    brace_depth += 1;
                } else if self.tokens[self.pos].token == Token::RBrace {
                    brace_depth -= 1;
                }
                self.pos += 1;
            }
        }
        Ok(())
    }

    fn check_type_parameters(&mut self) -> Result<(), ParseError> {
        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::Lt {
            let lt_info = self.tokens[self.pos].source_info.clone();
            let mut temp_pos = self.pos;
            while temp_pos < self.tokens.len() && self.tokens[temp_pos].token != Token::Gt {
                temp_pos += 1;
            }
            let end_info = if temp_pos < self.tokens.len() {
                self.tokens[temp_pos].source_info.clone()
            } else {
                lt_info.clone()
            };
            let src = SourceInfo::new(
                lt_info.source_id,
                lt_info.start_line,
                lt_info.start_column,
                end_info.end_line,
                end_info.end_column,
            );
            return Err(ParseError::EngineError(
                "Type and/or multiplicity parameters are not authorized in Legend Engine"
                    .to_string(),
                src,
            ));
        }
        Ok(())
    }

    fn parse_optional_extends(&mut self) -> Result<Vec<Type>, ParseError> {
        let mut super_types = Vec::new();
        if self.pos < self.tokens.len() && self.tokens[self.pos].token == Token::KeywordExtends {
            self.pos += 1;
            // stub parse super type path
            while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::LBrace {
                if let Some(id) = self.tokens[self.pos].token.as_ident() {
                    let id_info = self.tokens[self.pos].source_info.clone();
                    // Collect path logic omitted for prototype brevity, just grab last ident
                    super_types.push(Type::Packageable(PackageableType {
                        full_path: id.to_string(),
                        source_info: id_info,
                    }));
                    self.pos += 1;
                } else {
                    self.pos += 1;
                }
            }
        }
        Ok(super_types)
    }

    fn parse_profile(&mut self) -> Result<ProfileDef, ParseError> {
        let start_info = self.tokens[self.pos].source_info.clone();
        self.pos += 1; // Consume 'Profile'
        let mut pkg_path = Vec::new();
        let mut name = SmolStr::default();

        while self.pos < self.tokens.len() {
            if let Some(id) = self.tokens[self.pos].token.as_ident() {
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].token == Token::PathSep
                {
                    pkg_path.push(id.to_string());
                    self.pos += 2;
                } else {
                    name = id.clone().into();
                    self.pos += 1;
                    break;
                }
            } else {
                return Err(ParseError::UnexpectedToken(
                    self.tokens[self.pos].token.clone(),
                    self.pos,
                ));
            }
        }

        self.expect_token(Token::LBrace)?;

        let mut tags: Vec<legend_pure_parser_ast::ProfileTag> = Vec::new();
        let mut stereotypes: Vec<legend_pure_parser_ast::ProfileStereotype> = Vec::new();

        while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RBrace {
            if let Token::Ident(prop) = &self.tokens[self.pos].token {
                let is_tags = prop == "tags";
                let is_stereo = prop == "stereotypes";
                if is_tags || is_stereo {
                    self.pos += 1;
                    self.expect_token(Token::Colon)?;
                    self.expect_token(Token::LBracket)?;
                    while self.pos < self.tokens.len()
                        && self.tokens[self.pos].token != Token::RBracket
                    {
                        if let Token::Ident(val) = &self.tokens[self.pos].token {
                            let src_info = self.tokens[self.pos].source_info.clone();
                            if is_tags {
                                tags.push(legend_pure_parser_ast::ProfileTag {
                                    value: val.to_string(),
                                    source_info: src_info.clone(),
                                });
                            }
                            if is_stereo {
                                stereotypes.push(legend_pure_parser_ast::ProfileStereotype {
                                    value: val.to_string(),
                                    source_info: src_info,
                                });
                            }
                            self.pos += 1;
                        } else if self.tokens[self.pos].token == Token::Comma {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    self.expect_token(Token::RBracket)?;
                    self.expect_token(Token::Semi)?;
                } else {
                    self.pos += 1;
                }
            } else {
                self.pos += 1;
            }
        }

        let end_info = if self.pos < self.tokens.len() {
            self.tokens[self.pos].source_info.clone()
        } else {
            self.tokens.last().unwrap().source_info.clone()
        };
        self.expect_token(Token::RBrace)?;

        let source_info = SourceInfo {
            source_id: start_info.source_id.clone(),
            start_line: start_info.start_line,
            end_line: end_info.end_line,
            start_column: start_info.start_column,
            end_column: end_info.end_column,
        };

        Ok(ProfileDef {
            package: PackagePath {
                path: pkg_path,
                source_info: source_info.clone(),
            },
            name,
            tags,
            stereotypes,
            source_info,
        })
    }

    fn parse_property(&mut self) -> Result<Property, ParseError> {
        let stereotypes = self.parse_optional_stereotypes()?;
        let tagged_values = self.parse_optional_tagged_values()?;

        let name = if let Some(id) = self.tokens[self.pos].token.as_ident() {
            id.into()
        } else {
            return Err(ParseError::UnexpectedToken(
                self.tokens[self.pos].token.clone(),
                self.pos,
            ));
        };
        self.pos += 1;

        self.expect_token(Token::Colon)?;

        let mut type_path = Vec::new();
        while self.pos < self.tokens.len() {
            if let Some(id) = self.tokens[self.pos].token.as_ident() {
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].token == Token::PathSep
                {
                    type_path.push(id.to_string());
                    self.pos += 2;
                } else {
                    type_path.push(id.to_string());
                    self.pos += 1;
                    break;
                }
            } else {
                break;
            }
        }

        // Parse multiplicity `[1]`
        self.expect_token(Token::LBracket)?;
        // Simplification for prototype
        while self.pos < self.tokens.len() && self.tokens[self.pos].token != Token::RBracket {
            self.pos += 1;
        }
        self.expect_token(Token::RBracket)?;
        self.expect_token(Token::Semi)?;

        Ok(Property {
            name,
            property_type: Type::Packageable(PackageableType {
                full_path: type_path.join("::"),
                source_info: SourceInfo::dummy(),
            }),
            multiplicity: Multiplicity::pure_one(),
            stereotypes,
            tagged_values,
            source_info: SourceInfo::dummy(),
        })
    }

    pub(crate) fn expect_token(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.pos >= self.tokens.len() {
            return Err(ParseError::UnexpectedEof);
        }
        if self.tokens[self.pos].token == expected {
            self.pos += 1;
            Ok(())
        } else {
            Err(ParseError::UnexpectedToken(
                self.tokens[self.pos].token.clone(),
                self.pos,
            ))
        }
    }
}
