//! Lira Parser
//!
//! Parses tokens into an AST using recursive descent with Pratt parsing for expressions.

use crate::ast::*;
use crate::lexer::{Token, TokenKind};

/// The parser
pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    /// Collected parse errors for error recovery
    errors: Vec<String>,
    /// Whether we're in panic mode (after an error)
    panic_mode: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            current: 0,
            errors: Vec::new(),
            panic_mode: false,
        }
    }

    /// Record an error and enter panic mode
    fn error(&mut self, message: String) {
        if !self.panic_mode {
            self.errors.push(message);
            self.panic_mode = true;
        }
    }

    /// Synchronize after an error by advancing to a statement boundary
    fn synchronize(&mut self) {
        self.panic_mode = false;
        self.advance();

        while !self.is_at_end() {
            // If we just passed a semicolon, we're at a statement boundary
            if matches!(self.previous().kind, TokenKind::Semicolon) {
                return;
            }

            // If we see a keyword that starts a statement, stop
            match &self.peek().kind {
                TokenKind::Class
                | TokenKind::Fn
                | TokenKind::Let
                | TokenKind::Var
                | TokenKind::For
                | TokenKind::If
                | TokenKind::While
                | TokenKind::Return
                | TokenKind::Import
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Interface => return,
                _ => {}
            }

            self.advance();
        }
    }

    // Helper methods

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            return false;
        }
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn check_any(&self, kinds: &[TokenKind]) -> bool {
        kinds.iter().any(|k| self.check(k))
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    fn match_any(&mut self, kinds: &[TokenKind]) -> bool {
        for kind in kinds {
            if self.check(kind) {
                self.advance();
                return true;
            }
        }
        false
    }

    /// Check if the tokens after current position look like a struct literal start.
    /// A struct literal looks like: Name { field: value, ... }
    /// This is used to disambiguate from match expressions: match x { pattern => ... }
    fn is_struct_literal_start(&self) -> bool {
        // We're currently at the identifier, next should be {
        // We need to look at what comes after {
        if self.current + 1 >= self.tokens.len() {
            return false;
        }

        // Check if next token is {
        if !matches!(self.tokens[self.current].kind, TokenKind::LBrace) {
            return false;
        }

        // Look at token after {
        if self.current + 2 >= self.tokens.len() {
            return false;
        }

        let after_brace = &self.tokens[self.current + 1].kind;

        // If it's }, it's an empty struct literal
        if matches!(after_brace, TokenKind::RBrace) {
            return true;
        }

        // If it starts with an identifier, check if followed by :
        if let TokenKind::Identifier(_) = after_brace {
            if self.current + 3 < self.tokens.len() {
                // Check if the identifier is followed by :
                if matches!(self.tokens[self.current + 2].kind, TokenKind::Colon) {
                    return true;
                }
            }
        }

        // Otherwise, not a struct literal (could be match arms, block, etc.)
        false
    }

    fn consume(&mut self, kind: &TokenKind, message: &str) -> Result<&Token, String> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(format!(
                "{}:{}: {} (found {:?})",
                self.peek().line,
                self.peek().column,
                message,
                self.peek().kind
            ))
        }
    }

    fn span(&self) -> Span {
        Span::from(self.peek())
    }

    fn prev_span(&self) -> Span {
        Span::from(self.previous())
    }

    // Parsing methods

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut statements = Vec::new();

        while !self.is_at_end() {
            match self.declaration() {
                Ok(stmt) => statements.push(stmt),
                Err(msg) => {
                    self.error(msg);
                    self.synchronize();
                }
            }
        }

        if self.errors.is_empty() {
            Ok(Program { statements })
        } else {
            // Return all errors joined together
            Err(self.errors.join("\n"))
        }
    }

    /// Get the number of parse errors
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    fn declaration(&mut self) -> Result<Statement, String> {
        let span = self.span();

        // Check for visibility modifier
        let is_public = self.match_token(&TokenKind::Pub);

        if self.match_token(&TokenKind::Let) {
            self.var_declaration(false, span)
        } else if self.match_token(&TokenKind::Var) {
            self.var_declaration(true, span)
        } else if self.match_token(&TokenKind::Const) {
            self.const_declaration(span)
        } else if self.match_token(&TokenKind::Fn) {
            self.fn_declaration(is_public, false, span)
        } else if self.match_token(&TokenKind::Struct) {
            self.struct_declaration(span)
        } else if self.match_token(&TokenKind::Class) {
            self.class_declaration(span)
        } else if self.match_token(&TokenKind::Enum) {
            self.enum_declaration(span)
        } else if self.match_token(&TokenKind::Interface) {
            self.interface_declaration(span)
        } else if self.match_token(&TokenKind::Type) {
            self.type_alias(span)
        } else if self.match_token(&TokenKind::Import) {
            self.import_declaration(span)
        } else if self.match_token(&TokenKind::Trait) {
            self.trait_declaration(is_public, span)
        } else if self.match_token(&TokenKind::Impl) {
            self.impl_declaration(span)
        } else {
            self.statement()
        }
    }

    fn var_declaration(&mut self, mutable: bool, span: Span) -> Result<Statement, String> {
        // Parse pattern (supports destructuring: let (a, b) = ..., let { x, y } = ...)
        let pattern = self.binding_pattern()?;

        let type_ann = if self.match_token(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };

        let initializer = if self.match_token(&TokenKind::Eq) {
            Some(self.expression()?)
        } else {
            None
        };

        Ok(Statement {
            kind: StatementKind::VarDecl {
                pattern,
                mutable,
                type_ann,
                initializer,
            },
            span,
        })
    }

    /// Parse a binding pattern for let/var declarations
    /// Supports: identifier, (a, b), { x, y }
    fn binding_pattern(&mut self) -> Result<Pattern, String> {
        let span = self.span();

        // Tuple pattern: (a, b, c)
        if self.match_token(&TokenKind::LParen) {
            let mut patterns = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    patterns.push(self.binding_pattern()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "Expected ')' after tuple pattern")?;
            return Ok(Pattern {
                kind: PatternKind::Tuple(patterns),
                span,
            });
        }

        // Struct pattern: { x, y } or { x: a, y: b }
        if self.match_token(&TokenKind::LBrace) {
            let mut fields = Vec::new();
            if !self.check(&TokenKind::RBrace) {
                loop {
                    let field_name =
                        self.expect_identifier("Expected field name in struct pattern")?;
                    let field_pattern = if self.match_token(&TokenKind::Colon) {
                        self.binding_pattern()?
                    } else {
                        // Shorthand: { x } means { x: x }
                        Pattern {
                            kind: PatternKind::Variable(field_name.clone()),
                            span: self.span(),
                        }
                    };
                    fields.push((field_name, field_pattern));
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RBrace, "Expected '}' after struct pattern")?;
            return Ok(Pattern {
                kind: PatternKind::Struct {
                    name: String::new(), // Inferred from RHS
                    fields,
                    rest: false,
                },
                span,
            });
        }

        // Wildcard pattern: _
        if self.check(&TokenKind::Underscore)
            || (matches!(self.peek().kind, TokenKind::Identifier(_)) && self.peek().lexeme == "_")
        {
            self.advance();
            return Ok(Pattern {
                kind: PatternKind::Wildcard,
                span,
            });
        }

        // Simple variable binding
        let name = self.expect_identifier("Expected variable name or pattern")?;
        Ok(Pattern {
            kind: PatternKind::Variable(name),
            span,
        })
    }

    fn const_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected constant name")?;

        let type_ann = if self.match_token(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };

        self.consume(&TokenKind::Eq, "Expected '=' after constant name")?;
        let initializer = self.expression()?;

        Ok(Statement {
            kind: StatementKind::ConstDecl {
                name,
                type_ann,
                initializer,
            },
            span,
        })
    }

    fn fn_declaration(
        &mut self,
        is_public: bool,
        is_override: bool,
        span: Span,
    ) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected function name")?;

        // Parse optional generic type parameters <T> or <T, U>
        let type_params = if self.match_token(&TokenKind::Lt) {
            self.parse_type_params()?
        } else {
            Vec::new()
        };

        self.consume(&TokenKind::LParen, "Expected '(' after function name")?;
        let params = self.parameters()?;
        self.consume(&TokenKind::RParen, "Expected ')' after parameters")?;

        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };

        // Parse optional where clause: fn foo<T>() where T: Eq + Hash { }
        let mut type_params = type_params;
        if self.match_token(&TokenKind::Where) {
            self.parse_where_clause(&mut type_params)?;
        }

        // Check for expression body: fn foo() -> int => expr
        if self.match_token(&TokenKind::FatArrow) {
            let expr = self.expression()?;
            let body = Block {
                statements: vec![Statement {
                    kind: StatementKind::Return(Some(expr)),
                    span: span.clone(),
                }],
                span: span.clone(),
            };
            return Ok(Statement {
                kind: StatementKind::FnDecl {
                    name,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_public,
                    is_override,
                },
                span,
            });
        }

        let body = self.block()?;

        Ok(Statement {
            kind: StatementKind::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_public,
                is_override,
            },
            span,
        })
    }

    fn parameters(&mut self) -> Result<Vec<Parameter>, String> {
        let mut params = Vec::new();

        if !self.check(&TokenKind::RParen) {
            loop {
                let span = self.span();

                // Check for special 'self' parameter (no type annotation needed)
                if self.check(&TokenKind::This) || self.check(&TokenKind::Self_) {
                    self.advance();
                    // Check for 'self mut' or just 'self'
                    let _is_mut = self.match_token(&TokenKind::Mut);
                    params.push(Parameter {
                        name: "self".to_string(),
                        type_ann: TypeExpr {
                            kind: TypeExprKind::Named("Self".to_string()),
                            span: span.clone(),
                        },
                        default: None,
                        span,
                    });
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                let name = self.expect_identifier("Expected parameter name")?;

                // Check for 'self' written as identifier (in case it's not a keyword)
                if name == "self" && !self.check(&TokenKind::Colon) {
                    params.push(Parameter {
                        name: "self".to_string(),
                        type_ann: TypeExpr {
                            kind: TypeExprKind::Named("Self".to_string()),
                            span: span.clone(),
                        },
                        default: None,
                        span,
                    });
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                self.consume(&TokenKind::Colon, "Expected ':' after parameter name")?;
                let type_ann = self.type_expr()?;

                let default = if self.match_token(&TokenKind::Eq) {
                    Some(self.expression()?)
                } else {
                    None
                };

                params.push(Parameter {
                    name,
                    type_ann,
                    default,
                    span,
                });

                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }

        Ok(params)
    }

    /// Parse generic type parameters: <T> or <T, U> or <T: Trait>
    fn parse_type_params(&mut self) -> Result<Vec<TypeParam>, String> {
        let mut type_params = Vec::new();

        loop {
            let name = self.expect_identifier("Expected type parameter name")?;

            // Check for trait bounds: T: Trait or T: Trait + Other
            let bounds = if self.match_token(&TokenKind::Colon) {
                let mut bounds = Vec::new();
                loop {
                    bounds.push(self.expect_identifier("Expected trait name")?);
                    if !self.match_token(&TokenKind::Plus) {
                        break;
                    }
                }
                bounds
            } else {
                Vec::new()
            };

            type_params.push(TypeParam { name, bounds });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        self.consume(&TokenKind::Gt, "Expected '>' after type parameters")?;
        Ok(type_params)
    }

    /// Parse where clause bounds: where T: Eq + Hash, U: Display
    fn parse_where_clause(&mut self, type_params: &mut [TypeParam]) -> Result<(), String> {
        loop {
            let param_name =
                self.expect_identifier("Expected type parameter name in where clause")?;
            self.consume(
                &TokenKind::Colon,
                "Expected ':' after type parameter in where clause",
            )?;

            // Parse trait bounds: Trait or Trait + Other
            let mut bounds = Vec::new();
            loop {
                bounds.push(self.expect_identifier("Expected trait name")?);
                if !self.match_token(&TokenKind::Plus) {
                    break;
                }
            }

            // Find the type parameter and add the bounds
            let mut found = false;
            for tp in type_params.iter_mut() {
                if tp.name == param_name {
                    tp.bounds.extend(bounds.clone());
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(format!(
                    "Type parameter '{}' in where clause is not declared in the type parameter list",
                    param_name
                ));
            }

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        Ok(())
    }

    fn struct_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected struct name")?;

        // Parse optional generic type parameters <T> or <T, U>
        let type_params = if self.match_token(&TokenKind::Lt) {
            self.parse_type_params()?
        } else {
            Vec::new()
        };

        self.consume(&TokenKind::LBrace, "Expected '{' after struct name")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let member_span = self.span();
            let is_public = self.match_token(&TokenKind::Pub);

            if self.match_token(&TokenKind::Fn) {
                methods.push(self.fn_declaration(is_public, false, member_span)?);
            } else {
                let is_mutable = self.match_token(&TokenKind::Var);
                if !is_mutable {
                    self.match_token(&TokenKind::Let);
                }

                let field_name = self.expect_identifier("Expected field name")?;
                self.consume(&TokenKind::Colon, "Expected ':' after field name")?;
                let type_ann = self.type_expr()?;

                fields.push(Field {
                    name: field_name,
                    type_ann,
                    is_public,
                    is_mutable,
                    span: member_span,
                });
            }

            // Allow optional comma/newline between fields
            self.match_token(&TokenKind::Comma);
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after struct body")?;

        Ok(Statement {
            kind: StatementKind::StructDecl {
                name,
                type_params,
                fields,
                methods,
            },
            span,
        })
    }

    fn class_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected class name")?;

        // Parse inheritance: class Child extends Parent { } or class Child : Parent { }
        let parent = if self.match_token(&TokenKind::Extends) || self.match_token(&TokenKind::Colon)
        {
            Some(self.expect_identifier("Expected parent class name")?)
        } else {
            None
        };

        // Parse interfaces: class Child extends Parent, Interface1, Interface2 { }
        let mut interfaces = Vec::new();
        if parent.is_some() && self.match_token(&TokenKind::Comma) {
            loop {
                interfaces.push(self.expect_identifier("Expected interface name")?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::LBrace, "Expected '{' after class name")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let member_span = self.span();
            let is_public = self.match_token(&TokenKind::Pub);
            let _is_private = self.match_token(&TokenKind::Priv);
            let is_override = self.match_token(&TokenKind::Override);

            if self.match_token(&TokenKind::Fn) {
                methods.push(self.fn_declaration(is_public, is_override, member_span)?);
            } else {
                if is_override {
                    return Err("'override' can only be used on methods".to_string());
                }
                let is_mutable = self.match_token(&TokenKind::Var);
                if !is_mutable {
                    self.match_token(&TokenKind::Let);
                }

                let field_name = self.expect_identifier("Expected field name")?;
                self.consume(&TokenKind::Colon, "Expected ':' after field name")?;
                let type_ann = self.type_expr()?;

                fields.push(Field {
                    name: field_name,
                    type_ann,
                    is_public,
                    is_mutable,
                    span: member_span,
                });
            }
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after class body")?;

        Ok(Statement {
            kind: StatementKind::ClassDecl {
                name,
                parent,
                interfaces,
                fields,
                methods,
            },
            span,
        })
    }

    fn enum_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected enum name")?;
        self.consume(&TokenKind::LBrace, "Expected '{' after enum name")?;

        let mut variants = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let variant_span = self.span();
            let variant_name = self.expect_identifier("Expected variant name")?;

            let fields = if self.match_token(&TokenKind::LParen) {
                let mut types = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        types.push(self.type_expr()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, "Expected ')' after variant fields")?;
                types
            } else {
                Vec::new()
            };

            variants.push(EnumVariant {
                name: variant_name,
                fields,
                span: variant_span,
            });

            self.match_token(&TokenKind::Comma);
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after enum body")?;

        Ok(Statement {
            kind: StatementKind::EnumDecl { name, variants },
            span,
        })
    }

    fn interface_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected interface name")?;
        self.consume(&TokenKind::LBrace, "Expected '{' after interface name")?;

        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let method_span = self.span();
            self.consume(&TokenKind::Fn, "Expected 'fn' in interface")?;
            let method_name = self.expect_identifier("Expected method name")?;

            self.consume(&TokenKind::LParen, "Expected '(' after method name")?;
            let params = self.parameters()?;
            self.consume(&TokenKind::RParen, "Expected ')' after parameters")?;

            let return_type = if self.match_token(&TokenKind::Arrow) {
                Some(self.type_expr()?)
            } else {
                None
            };

            methods.push(InterfaceMethod {
                name: method_name,
                params,
                return_type,
                span: method_span,
            });
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after interface body")?;

        Ok(Statement {
            kind: StatementKind::InterfaceDecl { name, methods },
            span,
        })
    }

    fn trait_declaration(&mut self, is_public: bool, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected trait name")?;

        // Parse optional type parameters: trait Into<T>
        let type_params = if self.match_token(&TokenKind::Lt) {
            self.parse_type_params()?
        } else {
            Vec::new()
        };

        // TODO: Parse supertraits: trait Ord: Eq { }

        self.consume(&TokenKind::LBrace, "Expected '{' after trait name")?;

        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let method_span = self.span();
            self.consume(&TokenKind::Fn, "Expected 'fn' in trait")?;
            let method_name = self.expect_identifier("Expected method name")?;

            self.consume(&TokenKind::LParen, "Expected '(' after method name")?;

            // Parse parameters, checking for self
            let mut has_self = false;
            let mut params = Vec::new();

            if !self.check(&TokenKind::RParen) {
                // Check if first param is self
                if self.check(&TokenKind::Self_) {
                    has_self = true;
                    self.advance(); // consume 'self'

                    // Check for 'self mut'
                    if self.match_token(&TokenKind::Mut) {
                        // self mut - mutability handled
                    }

                    // If there are more params, consume comma
                    if self.check(&TokenKind::Comma) {
                        self.advance();
                    }
                }

                // Parse remaining parameters
                if !self.check(&TokenKind::RParen) {
                    params = self.parameters()?;
                }
            }

            self.consume(&TokenKind::RParen, "Expected ')' after parameters")?;

            let return_type = if self.match_token(&TokenKind::Arrow) {
                Some(self.type_expr()?)
            } else {
                None
            };

            // Check for default implementation
            let default_impl = if self.check(&TokenKind::LBrace) {
                Some(self.block()?)
            } else {
                None
            };

            methods.push(TraitMethod {
                name: method_name,
                params,
                return_type,
                has_self,
                default_impl,
                span: method_span,
            });
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after trait body")?;

        Ok(Statement {
            kind: StatementKind::TraitDecl {
                name,
                type_params,
                methods,
                is_public,
            },
            span,
        })
    }

    fn impl_declaration(&mut self, span: Span) -> Result<Statement, String> {
        // Parse optional type parameters: impl<T>
        let type_params = if self.match_token(&TokenKind::Lt) {
            self.parse_type_params()?
        } else {
            Vec::new()
        };

        // Parse first identifier - could be trait name or type name
        let first_name = self.expect_type_name("Expected type or trait name")?;

        // Check if this is "impl Trait for Type" or just "impl Type"
        let (trait_name, type_name) = if self.match_token(&TokenKind::For) {
            // This is "impl Trait for Type"
            let target_type = self.expect_type_name("Expected type name after 'for'")?;
            // Skip generic args if present (List<T>)
            if self.match_token(&TokenKind::Lt) {
                while !self.check(&TokenKind::Gt) && !self.is_at_end() {
                    self.advance();
                }
                self.consume(&TokenKind::Gt, "Expected '>' after type parameters")?;
            }
            (Some(first_name), target_type)
        } else {
            // This is just "impl Type" - check for generic args like List<T>
            // For now, just use the identifier
            if self.match_token(&TokenKind::Lt) {
                // Skip generic args for now (List<T>)
                while !self.check(&TokenKind::Gt) && !self.is_at_end() {
                    self.advance();
                }
                self.consume(&TokenKind::Gt, "Expected '>' after type parameters")?;
            }
            (None, first_name)
        };

        self.consume(&TokenKind::LBrace, "Expected '{' after impl declaration")?;

        let mut methods = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let method_span = self.span();

            // Parse optional visibility
            let is_public = self.match_token(&TokenKind::Pub);

            if self.match_token(&TokenKind::Fn) {
                let fn_decl = self.fn_declaration(is_public, false, method_span)?;
                methods.push(fn_decl);
            } else {
                let span = self.span();
                return Err(format!(
                    "{}:{}: Expected 'fn' in impl block",
                    span.line, span.column
                ));
            }
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after impl body")?;

        Ok(Statement {
            kind: StatementKind::ImplDecl {
                trait_name,
                type_name,
                type_params,
                methods,
            },
            span,
        })
    }

    fn type_alias(&mut self, span: Span) -> Result<Statement, String> {
        let name = self.expect_identifier("Expected type name")?;
        self.consume(&TokenKind::Eq, "Expected '=' after type name")?;
        let type_expr = self.type_expr()?;

        Ok(Statement {
            kind: StatementKind::TypeAlias { name, type_expr },
            span,
        })
    }

    fn import_declaration(&mut self, span: Span) -> Result<Statement, String> {
        let mut path = vec![self.expect_identifier("Expected module name")?];

        while self.match_token(&TokenKind::Dot) {
            if self.match_token(&TokenKind::LBrace) {
                // import std.io.{File, Dir}
                let mut items = Vec::new();
                loop {
                    items.push(self.expect_identifier("Expected import item")?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.consume(&TokenKind::RBrace, "Expected '}' after import items")?;
                return Ok(Statement {
                    kind: StatementKind::Import {
                        path,
                        items: Some(items),
                    },
                    span,
                });
            }
            path.push(self.expect_identifier("Expected module name")?);
        }

        Ok(Statement {
            kind: StatementKind::Import { path, items: None },
            span,
        })
    }

    fn statement(&mut self) -> Result<Statement, String> {
        let span = self.span();

        if self.match_token(&TokenKind::If) {
            self.if_statement(span)
        } else if self.match_token(&TokenKind::While) {
            self.while_statement(span)
        } else if self.match_token(&TokenKind::For) {
            self.for_statement(span)
        } else if self.match_token(&TokenKind::Loop) {
            self.loop_statement(span)
        } else if self.match_token(&TokenKind::Return) {
            self.return_statement(span)
        } else if self.match_token(&TokenKind::Break) {
            self.break_statement(span)
        } else if self.match_token(&TokenKind::Continue) {
            Ok(Statement {
                kind: StatementKind::Continue,
                span,
            })
        } else if self.check(&TokenKind::LBrace) {
            let block = self.block()?;
            Ok(Statement {
                kind: StatementKind::Block(block),
                span,
            })
        } else {
            self.expression_statement(span)
        }
    }

    fn if_statement(&mut self, span: Span) -> Result<Statement, String> {
        let condition = self.expression()?;
        let then_branch = self.block()?;

        let else_branch = if self.match_token(&TokenKind::Else) {
            if self.check(&TokenKind::If) {
                let else_span = self.span();
                self.advance(); // consume 'if'
                let else_if = self.if_statement(else_span.clone())?;
                Some(Block {
                    statements: vec![else_if],
                    span: else_span,
                })
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };

        Ok(Statement {
            kind: StatementKind::If {
                condition,
                then_branch,
                else_branch,
            },
            span,
        })
    }

    fn while_statement(&mut self, span: Span) -> Result<Statement, String> {
        let condition = self.expression()?;
        let body = self.block()?;

        Ok(Statement {
            kind: StatementKind::While { condition, body },
            span,
        })
    }

    fn for_statement(&mut self, span: Span) -> Result<Statement, String> {
        let variable = self.expect_identifier("Expected loop variable")?;
        self.consume(&TokenKind::In, "Expected 'in' after loop variable")?;
        let iterable = self.expression()?;
        let body = self.block()?;

        Ok(Statement {
            kind: StatementKind::For {
                variable,
                iterable,
                body,
            },
            span,
        })
    }

    fn loop_statement(&mut self, span: Span) -> Result<Statement, String> {
        let body = self.block()?;
        Ok(Statement {
            kind: StatementKind::Loop { body },
            span,
        })
    }

    fn return_statement(&mut self, span: Span) -> Result<Statement, String> {
        let value = if self.check(&TokenKind::RBrace) || self.check(&TokenKind::Eof) {
            None
        } else {
            Some(self.expression()?)
        };

        Ok(Statement {
            kind: StatementKind::Return(value),
            span,
        })
    }

    fn break_statement(&mut self, span: Span) -> Result<Statement, String> {
        let value = if self.check(&TokenKind::RBrace) || self.check(&TokenKind::Eof) {
            None
        } else if !self.check_any(&[TokenKind::If, TokenKind::While, TokenKind::For]) {
            Some(self.expression()?)
        } else {
            None
        };

        Ok(Statement {
            kind: StatementKind::Break(value),
            span,
        })
    }

    fn expression_statement(&mut self, span: Span) -> Result<Statement, String> {
        let expr = self.expression()?;
        Ok(Statement {
            kind: StatementKind::Expression(expr),
            span,
        })
    }

    fn block(&mut self) -> Result<Block, String> {
        let span = self.span();
        self.consume(&TokenKind::LBrace, "Expected '{'")?;

        let mut statements = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            statements.push(self.declaration()?);
        }

        self.consume(&TokenKind::RBrace, "Expected '}'")?;

        Ok(Block { statements, span })
    }

    // Type expressions

    fn type_expr(&mut self) -> Result<TypeExpr, String> {
        let span = self.span();

        // Function type: fn(A, B) -> C
        if self.match_token(&TokenKind::Fn) {
            self.consume(&TokenKind::LParen, "Expected '(' after 'fn'")?;
            let mut params = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    params.push(self.type_expr()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "Expected ')'")?;
            self.consume(&TokenKind::Arrow, "Expected '->' in function type")?;
            let return_type = Box::new(self.type_expr()?);
            return Ok(TypeExpr {
                kind: TypeExprKind::Function {
                    params,
                    return_type,
                },
                span,
            });
        }

        // Tuple type: (A, B, C)
        if self.match_token(&TokenKind::LParen) {
            let mut types = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    types.push(self.type_expr()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "Expected ')'")?;
            return Ok(TypeExpr {
                kind: TypeExprKind::Tuple(types),
                span,
            });
        }

        // Array type: [T]
        if self.match_token(&TokenKind::LBracket) {
            let element_type = self.type_expr()?;
            self.consume(
                &TokenKind::RBracket,
                "Expected ']' after array element type",
            )?;
            return Ok(TypeExpr {
                kind: TypeExprKind::Array(Box::new(element_type)),
                span,
            });
        }

        // Self type
        if self.match_token(&TokenKind::SelfType) {
            return Ok(TypeExpr {
                kind: TypeExprKind::Named("Self".to_string()),
                span,
            });
        }

        // Named type or generic
        let name = self.expect_type_name("Expected type name")?;

        let kind = if self.match_token(&TokenKind::Lt) {
            // Generic type: List<T>
            let mut args = Vec::new();
            loop {
                args.push(self.type_expr()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            self.consume(&TokenKind::Gt, "Expected '>' after type arguments")?;
            TypeExprKind::Generic { name, args }
        } else {
            TypeExprKind::Named(name)
        };

        let mut result = TypeExpr {
            kind,
            span: span.clone(),
        };

        // Optional type: T?
        if self.match_token(&TokenKind::Question) {
            result = TypeExpr {
                kind: TypeExprKind::Optional(Box::new(result)),
                span,
            };
        }

        Ok(result)
    }

    // Expression parsing using Pratt parsing

    fn expression(&mut self) -> Result<Expression, String> {
        self.parse_precedence(Precedence::Assignment)
    }

    fn parse_precedence(&mut self, precedence: Precedence) -> Result<Expression, String> {
        let mut left = self.prefix()?;

        while precedence <= self.current_precedence() {
            left = self.infix(left)?;
        }

        Ok(left)
    }

    fn current_precedence(&self) -> Precedence {
        match &self.peek().kind {
            TokenKind::Eq
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq
            | TokenKind::AmpEq
            | TokenKind::PipeEq
            | TokenKind::CaretEq
            | TokenKind::LtLtEq
            | TokenKind::GtGtEq => Precedence::Assignment,
            TokenKind::PipePipe => Precedence::Or,
            TokenKind::AmpAmp => Precedence::And,
            TokenKind::Pipe => Precedence::BitOr,
            TokenKind::Caret => Precedence::BitXor,
            TokenKind::Amp => Precedence::BitAnd,
            TokenKind::EqEq | TokenKind::BangEq => Precedence::Equality,
            TokenKind::Lt | TokenKind::Le | TokenKind::Gt | TokenKind::Ge => Precedence::Comparison,
            TokenKind::LtLt | TokenKind::GtGt | TokenKind::GtGtGt => Precedence::Shift,
            TokenKind::Plus | TokenKind::Minus => Precedence::Term,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Precedence::Factor,
            TokenKind::StarStar => Precedence::Power,
            TokenKind::As | TokenKind::Is => Precedence::Cast,
            TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::Dot
            | TokenKind::QuestionDot
            | TokenKind::Bang
            | TokenKind::Question
            | TokenKind::PlusPlus
            | TokenKind::MinusMinus => Precedence::Call,
            TokenKind::QuestionQuestion => Precedence::NullCoalesce,
            TokenKind::DotDot | TokenKind::DotDotEq => Precedence::Range,
            _ => Precedence::None,
        }
    }

    fn prefix(&mut self) -> Result<Expression, String> {
        let span = self.span();
        let token = self.advance().clone();

        match &token.kind {
            TokenKind::IntLiteral(n) => Ok(Expression {
                kind: ExpressionKind::IntLiteral(*n),
                span,
            }),
            TokenKind::FloatLiteral(n) => Ok(Expression {
                kind: ExpressionKind::FloatLiteral(*n),
                span,
            }),
            TokenKind::StringLiteral(s) => Ok(Expression {
                kind: ExpressionKind::StringLiteral(s.clone()),
                span,
            }),
            TokenKind::CharLiteral(c) => Ok(Expression {
                kind: ExpressionKind::CharLiteral(*c),
                span,
            }),
            TokenKind::BoolLiteral(b) => Ok(Expression {
                kind: ExpressionKind::BoolLiteral(*b),
                span,
            }),
            TokenKind::Null => Ok(Expression {
                kind: ExpressionKind::Null,
                span,
            }),
            TokenKind::Identifier(name) => {
                let name = name.clone();

                // Check if this is an enum variant: Name::Variant
                if self.check(&TokenKind::ColonColon) {
                    self.advance(); // consume ::
                    let variant_name =
                        self.expect_identifier("Expected variant name after '::'")?;
                    return Ok(Expression {
                        kind: ExpressionKind::EnumVariant {
                            enum_name: name,
                            variant_name,
                        },
                        span,
                    });
                }

                // Check if this is a struct literal: Identifier { field: value, ... }
                // Only parse as struct literal if the first token after { looks like a field name
                // (identifier followed by :), not like a statement or match arm
                if self.check(&TokenKind::LBrace) {
                    // Look ahead to see if this is really a struct literal
                    // Struct literal: Name { field: value, ... }
                    // Not struct literal: match x { 0 => ..., ... } (match body)
                    // We peek at token after { to disambiguate
                    if self.is_struct_literal_start() {
                        self.struct_literal(name, span)
                    } else {
                        Ok(Expression {
                            kind: ExpressionKind::Identifier(name),
                            span,
                        })
                    }
                } else {
                    Ok(Expression {
                        kind: ExpressionKind::Identifier(name),
                        span,
                    })
                }
            }
            TokenKind::This => Ok(Expression {
                kind: ExpressionKind::Identifier("this".to_string()),
                span,
            }),
            TokenKind::Super => Ok(Expression {
                kind: ExpressionKind::Identifier("super".to_string()),
                span,
            }),
            TokenKind::Self_ => Ok(Expression {
                kind: ExpressionKind::Identifier("self".to_string()),
                span,
            }),
            TokenKind::Minus => {
                let operand = self.parse_precedence(Precedence::Unary)?;
                Ok(Expression {
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::Bang => {
                let operand = self.parse_precedence(Precedence::Unary)?;
                Ok(Expression {
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::Tilde => {
                let operand = self.parse_precedence(Precedence::Unary)?;
                Ok(Expression {
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::BitNot,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::PlusPlus => {
                // Prefix increment: ++x
                let operand = self.parse_precedence(Precedence::Unary)?;
                Ok(Expression {
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::PreInc,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::MinusMinus => {
                // Prefix decrement: --x
                let operand = self.parse_precedence(Precedence::Unary)?;
                Ok(Expression {
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::PreDec,
                        operand: Box::new(operand),
                    },
                    span,
                })
            }
            TokenKind::LParen => {
                // Tuple or grouped expression
                let expr = self.expression()?;
                if self.match_token(&TokenKind::Comma) {
                    // Tuple
                    let mut elements = vec![expr];
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            elements.push(self.expression()?);
                            if !self.match_token(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.consume(&TokenKind::RParen, "Expected ')'")?;
                    Ok(Expression {
                        kind: ExpressionKind::Tuple(elements),
                        span,
                    })
                } else {
                    self.consume(&TokenKind::RParen, "Expected ')'")?;
                    Ok(expr)
                }
            }
            TokenKind::LBracket => {
                // Array literal
                let mut elements = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.expression()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RBracket, "Expected ']'")?;
                Ok(Expression {
                    kind: ExpressionKind::Array(elements),
                    span,
                })
            }
            TokenKind::LBrace => {
                // Block expression or struct literal
                self.current -= 1; // Put back the brace
                let block = self.block()?;
                Ok(Expression {
                    kind: ExpressionKind::Block(block),
                    span,
                })
            }
            TokenKind::If => self.if_expression(span),
            TokenKind::Match => self.match_expression(span),
            TokenKind::Pipe => self.lambda_expression(span),
            TokenKind::PipePipe => {
                // Empty lambda: || expr
                let body = self.expression()?;
                Ok(Expression {
                    kind: ExpressionKind::Lambda {
                        params: Vec::new(),
                        body: Box::new(body),
                    },
                    span,
                })
            }
            TokenKind::Spawn => {
                let expr = self.expression()?;
                Ok(Expression {
                    kind: ExpressionKind::Spawn(Box::new(expr)),
                    span,
                })
            }
            TokenKind::Select => self.select_expression(span),
            _ => Err(format!(
                "{}:{}: Unexpected token: {:?}",
                token.line, token.column, token.kind
            )),
        }
    }

    fn infix(&mut self, left: Expression) -> Result<Expression, String> {
        let span = left.span.clone();
        let token = self.advance().clone();

        match &token.kind {
            // Binary operators
            TokenKind::Plus => self.binary(left, BinaryOp::Add, Precedence::Term, span),
            TokenKind::Minus => self.binary(left, BinaryOp::Sub, Precedence::Term, span),
            TokenKind::Star => self.binary(left, BinaryOp::Mul, Precedence::Factor, span),
            TokenKind::Slash => self.binary(left, BinaryOp::Div, Precedence::Factor, span),
            TokenKind::Percent => self.binary(left, BinaryOp::Mod, Precedence::Factor, span),
            TokenKind::StarStar => self.binary(left, BinaryOp::Pow, Precedence::Power, span),
            TokenKind::EqEq => self.binary(left, BinaryOp::Eq, Precedence::Equality, span),
            TokenKind::BangEq => self.binary(left, BinaryOp::Ne, Precedence::Equality, span),
            TokenKind::Lt => self.binary(left, BinaryOp::Lt, Precedence::Comparison, span),
            TokenKind::Le => self.binary(left, BinaryOp::Le, Precedence::Comparison, span),
            TokenKind::Gt => self.binary(left, BinaryOp::Gt, Precedence::Comparison, span),
            TokenKind::Ge => self.binary(left, BinaryOp::Ge, Precedence::Comparison, span),
            TokenKind::AmpAmp => self.binary(left, BinaryOp::And, Precedence::And, span),
            TokenKind::PipePipe => self.binary(left, BinaryOp::Or, Precedence::Or, span),
            TokenKind::Amp => self.binary(left, BinaryOp::BitAnd, Precedence::BitAnd, span),
            TokenKind::Pipe => self.binary(left, BinaryOp::BitOr, Precedence::BitOr, span),
            TokenKind::Caret => self.binary(left, BinaryOp::BitXor, Precedence::BitXor, span),
            TokenKind::LtLt => self.binary(left, BinaryOp::Shl, Precedence::Shift, span),
            TokenKind::GtGt => self.binary(left, BinaryOp::Shr, Precedence::Shift, span),
            TokenKind::GtGtGt => self.binary(left, BinaryOp::UShr, Precedence::Shift, span),
            TokenKind::QuestionQuestion => {
                self.binary(left, BinaryOp::NullCoalesce, Precedence::NullCoalesce, span)
            }

            // Assignment
            TokenKind::Eq => {
                let right = self.parse_precedence(Precedence::Assignment)?;
                Ok(Expression {
                    kind: ExpressionKind::Assign {
                        target: Box::new(left),
                        value: Box::new(right),
                    },
                    span,
                })
            }
            TokenKind::PlusEq => self.compound_assign(left, BinaryOp::Add, span),
            TokenKind::MinusEq => self.compound_assign(left, BinaryOp::Sub, span),
            TokenKind::StarEq => self.compound_assign(left, BinaryOp::Mul, span),
            TokenKind::SlashEq => self.compound_assign(left, BinaryOp::Div, span),
            TokenKind::PercentEq => self.compound_assign(left, BinaryOp::Mod, span),
            TokenKind::AmpEq => self.compound_assign(left, BinaryOp::BitAnd, span),
            TokenKind::PipeEq => self.compound_assign(left, BinaryOp::BitOr, span),
            TokenKind::CaretEq => self.compound_assign(left, BinaryOp::BitXor, span),
            TokenKind::LtLtEq => self.compound_assign(left, BinaryOp::Shl, span),
            TokenKind::GtGtEq => self.compound_assign(left, BinaryOp::Shr, span),

            // Call (with named argument support)
            TokenKind::LParen => {
                let args = self.parse_call_arguments()?;
                self.consume(&TokenKind::RParen, "Expected ')' after arguments")?;
                Ok(Expression {
                    kind: ExpressionKind::Call {
                        callee: Box::new(left),
                        type_args: Vec::new(), // TODO: Parse turbofish syntax ::<T>
                        args,
                    },
                    span,
                })
            }

            // Index
            TokenKind::LBracket => {
                let index = self.expression()?;
                self.consume(&TokenKind::RBracket, "Expected ']'")?;
                Ok(Expression {
                    kind: ExpressionKind::Index {
                        object: Box::new(left),
                        index: Box::new(index),
                    },
                    span,
                })
            }

            // Field access
            TokenKind::Dot => {
                let field = self.expect_identifier("Expected field name")?;
                Ok(Expression {
                    kind: ExpressionKind::FieldAccess {
                        object: Box::new(left),
                        field,
                    },
                    span,
                })
            }

            // Optional chaining
            TokenKind::QuestionDot => {
                let field = self.expect_identifier("Expected field name")?;
                Ok(Expression {
                    kind: ExpressionKind::OptionalAccess {
                        object: Box::new(left),
                        field,
                    },
                    span,
                })
            }

            // Postfix increment: x++
            TokenKind::PlusPlus => Ok(Expression {
                kind: ExpressionKind::Unary {
                    op: UnaryOp::PostInc,
                    operand: Box::new(left),
                },
                span,
            }),

            // Postfix decrement: x--
            TokenKind::MinusMinus => Ok(Expression {
                kind: ExpressionKind::Unary {
                    op: UnaryOp::PostDec,
                    operand: Box::new(left),
                },
                span,
            }),

            // Type cast
            TokenKind::As => {
                let type_expr = self.type_expr()?;
                Ok(Expression {
                    kind: ExpressionKind::Cast {
                        expr: Box::new(left),
                        type_expr,
                    },
                    span,
                })
            }

            // Type check
            TokenKind::Is => {
                let type_expr = self.type_expr()?;
                Ok(Expression {
                    kind: ExpressionKind::TypeCheck {
                        expr: Box::new(left),
                        type_expr,
                    },
                    span,
                })
            }

            // Range
            TokenKind::DotDot => {
                let end = if self.current_precedence() > Precedence::Range {
                    Some(Box::new(self.parse_precedence(Precedence::Range)?))
                } else if !self.check(&TokenKind::RBracket)
                    && !self.check(&TokenKind::RParen)
                    && !self.check(&TokenKind::Comma)
                    && !self.check(&TokenKind::LBrace)
                {
                    Some(Box::new(self.parse_precedence(Precedence::Term)?))
                } else {
                    None
                };
                Ok(Expression {
                    kind: ExpressionKind::Range {
                        start: Some(Box::new(left)),
                        end,
                        inclusive: false,
                    },
                    span,
                })
            }

            TokenKind::DotDotEq => {
                let end = Some(Box::new(self.parse_precedence(Precedence::Term)?));
                Ok(Expression {
                    kind: ExpressionKind::Range {
                        start: Some(Box::new(left)),
                        end,
                        inclusive: true,
                    },
                    span,
                })
            }

            // Try/error propagation: expr?
            TokenKind::Question => Ok(Expression {
                kind: ExpressionKind::Try(Box::new(left)),
                span,
            }),

            _ => Err(format!(
                "{}:{}: Unexpected infix token: {:?}",
                token.line, token.column, token.kind
            )),
        }
    }

    fn binary(
        &mut self,
        left: Expression,
        op: BinaryOp,
        prec: Precedence,
        span: Span,
    ) -> Result<Expression, String> {
        let right = self.parse_precedence(prec.next())?;
        Ok(Expression {
            kind: ExpressionKind::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
            span,
        })
    }

    fn compound_assign(
        &mut self,
        left: Expression,
        op: BinaryOp,
        span: Span,
    ) -> Result<Expression, String> {
        let right = self.parse_precedence(Precedence::Assignment)?;
        Ok(Expression {
            kind: ExpressionKind::CompoundAssign {
                target: Box::new(left),
                op,
                value: Box::new(right),
            },
            span,
        })
    }

    fn if_expression(&mut self, span: Span) -> Result<Expression, String> {
        let condition = self.expression()?;
        let then_block = self.block()?;

        self.consume(&TokenKind::Else, "Expected 'else' in if expression")?;

        let else_expr = if self.check(&TokenKind::If) {
            self.advance();
            self.if_expression(self.prev_span())?
        } else {
            let else_block = self.block()?;
            Expression {
                kind: ExpressionKind::Block(else_block),
                span: span.clone(),
            }
        };

        Ok(Expression {
            kind: ExpressionKind::IfExpr {
                condition: Box::new(condition),
                then_expr: Box::new(Expression {
                    kind: ExpressionKind::Block(then_block),
                    span: span.clone(),
                }),
                else_expr: Box::new(else_expr),
            },
            span,
        })
    }

    fn match_expression(&mut self, span: Span) -> Result<Expression, String> {
        let subject = self.expression()?;
        self.consume(&TokenKind::LBrace, "Expected '{' after match subject")?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let arm_span = self.span();
            let pattern = self.pattern()?;

            let guard = if self.match_token(&TokenKind::If) {
                Some(self.expression()?)
            } else {
                None
            };

            self.consume(&TokenKind::FatArrow, "Expected '=>' after pattern")?;
            let body = self.expression()?;

            arms.push(MatchArm {
                pattern,
                guard,
                body,
                span: arm_span,
            });

            self.match_token(&TokenKind::Comma);
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after match arms")?;

        Ok(Expression {
            kind: ExpressionKind::Match {
                subject: Box::new(subject),
                arms,
            },
            span,
        })
    }

    fn struct_literal(&mut self, name: String, span: Span) -> Result<Expression, String> {
        self.consume(&TokenKind::LBrace, "Expected '{' after struct name")?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            // Field name
            let field_name = self.expect_identifier("Expected field name")?;

            // Colon
            self.consume(&TokenKind::Colon, "Expected ':' after field name")?;

            // Field value
            let value = self.expression()?;

            fields.push((field_name, value));

            // Optional comma
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after struct fields")?;

        Ok(Expression {
            kind: ExpressionKind::StructLiteral {
                name: Some(name),
                fields,
            },
            span,
        })
    }

    fn select_expression(&mut self, span: Span) -> Result<Expression, String> {
        self.consume(&TokenKind::LBrace, "Expected '{' after select")?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let arm_span = self.span();

            // Check for default case: _ => ...
            if self.check(&TokenKind::Identifier("_".to_string())) {
                self.advance();
                self.consume(&TokenKind::FatArrow, "Expected '=>' after '_'")?;
                let body = self.expression()?;
                arms.push(SelectArm {
                    kind: SelectArmKind::Default,
                    body,
                    span: arm_span,
                });
                self.match_token(&TokenKind::Comma);
                continue;
            }

            // Check for receive: <-channel or variable = <-channel
            if self.match_token(&TokenKind::LtMinus) {
                // <-channel => ...
                let channel = self.expression()?;
                self.consume(&TokenKind::FatArrow, "Expected '=>' after channel")?;
                let body = self.expression()?;
                arms.push(SelectArm {
                    kind: SelectArmKind::Recv {
                        variable: None,
                        channel,
                    },
                    body,
                    span: arm_span,
                });
                self.match_token(&TokenKind::Comma);
                continue;
            }

            // Could be: variable = <-channel => ... or value -> channel => ...
            let first_expr = self.expression()?;

            if self.match_token(&TokenKind::Arrow) {
                // value -> channel => ... (send)
                let channel = self.expression()?;
                self.consume(&TokenKind::FatArrow, "Expected '=>' after channel")?;
                let body = self.expression()?;
                arms.push(SelectArm {
                    kind: SelectArmKind::Send {
                        value: first_expr,
                        channel,
                    },
                    body,
                    span: arm_span,
                });
            } else if self.match_token(&TokenKind::Eq) {
                // variable = <-channel => ...
                self.consume(&TokenKind::LtMinus, "Expected '<-' after '='")?;
                let channel = self.expression()?;

                // Extract variable name from expression
                let variable = match first_expr.kind {
                    ExpressionKind::Identifier(name) => name,
                    _ => {
                        return Err(format!(
                            "{}:{}: Expected identifier before '='",
                            arm_span.line, arm_span.column
                        ));
                    }
                };

                self.consume(&TokenKind::FatArrow, "Expected '=>' after channel")?;
                let body = self.expression()?;
                arms.push(SelectArm {
                    kind: SelectArmKind::Recv {
                        variable: Some(variable),
                        channel,
                    },
                    body,
                    span: arm_span,
                });
            } else {
                return Err(format!(
                    "{}:{}: Expected '->' or '=' in select arm",
                    arm_span.line, arm_span.column
                ));
            }

            self.match_token(&TokenKind::Comma);
        }

        self.consume(&TokenKind::RBrace, "Expected '}' after select arms")?;

        Ok(Expression {
            kind: ExpressionKind::Select(arms),
            span,
        })
    }

    fn lambda_expression(&mut self, span: Span) -> Result<Expression, String> {
        // |params| body
        let mut params = Vec::new();
        if !self.check(&TokenKind::Pipe) {
            loop {
                let param_span = self.span();
                let name = self.expect_identifier("Expected parameter name")?;
                let type_ann = if self.match_token(&TokenKind::Colon) {
                    self.type_expr()?
                } else {
                    // Infer type
                    TypeExpr {
                        kind: TypeExprKind::Named("_".to_string()),
                        span: param_span.clone(),
                    }
                };
                params.push(Parameter {
                    name,
                    type_ann,
                    default: None,
                    span: param_span,
                });
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::Pipe, "Expected '|' after lambda parameters")?;

        let body = self.expression()?;

        Ok(Expression {
            kind: ExpressionKind::Lambda {
                params,
                body: Box::new(body),
            },
            span,
        })
    }

    fn pattern(&mut self) -> Result<Pattern, String> {
        let span = self.span();

        // Wildcard - must check actual identifier value, not just variant
        if let TokenKind::Identifier(name) = &self.peek().kind {
            if name == "_" {
                self.advance();
                return Ok(Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                });
            }
        }

        // Literal patterns
        match &self.peek().kind {
            TokenKind::IntLiteral(_)
            | TokenKind::FloatLiteral(_)
            | TokenKind::StringLiteral(_)
            | TokenKind::CharLiteral(_)
            | TokenKind::BoolLiteral(_)
            | TokenKind::Null => {
                let expr = self.prefix()?;
                return Ok(Pattern {
                    kind: PatternKind::Literal(expr),
                    span,
                });
            }
            _ => {}
        }

        // Variable or constructor
        if let TokenKind::Identifier(name) = &self.peek().kind.clone() {
            let name = name.clone();
            self.advance();

            // Check for enum variant pattern: Color::Red
            if self.match_token(&TokenKind::ColonColon) {
                let variant_name = self.expect_identifier("Expected variant name after '::'")?;
                let full_name = format!("{}::{}", name, variant_name);

                // Check for associated data: Color::RGB(r, g, b)
                if self.match_token(&TokenKind::LParen) {
                    let mut fields = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            fields.push(self.pattern()?);
                            if !self.match_token(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.consume(&TokenKind::RParen, "Expected ')'")?;
                    return Ok(Pattern {
                        kind: PatternKind::Constructor {
                            name: full_name,
                            fields,
                        },
                        span,
                    });
                }

                // Unit variant: Color::Red
                return Ok(Pattern {
                    kind: PatternKind::Constructor {
                        name: full_name,
                        fields: vec![],
                    },
                    span,
                });
            }

            if self.match_token(&TokenKind::LParen) {
                // Constructor pattern: Some(x)
                let mut fields = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        fields.push(self.pattern()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, "Expected ')'")?;
                return Ok(Pattern {
                    kind: PatternKind::Constructor { name, fields },
                    span,
                });
            }

            // Variable binding
            return Ok(Pattern {
                kind: PatternKind::Variable(name),
                span,
            });
        }

        // Tuple pattern
        if self.match_token(&TokenKind::LParen) {
            let mut patterns = Vec::new();
            if !self.check(&TokenKind::RParen) {
                loop {
                    patterns.push(self.pattern()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "Expected ')'")?;
            return Ok(Pattern {
                kind: PatternKind::Tuple(patterns),
                span,
            });
        }

        Err(format!("{}:{}: Expected pattern", span.line, span.column))
    }

    // Helper methods for identifiers and type names

    /// Parse function call arguments, supporting both positional and named arguments.
    /// Named arguments use the syntax: `name: value`
    /// Positional arguments must come before named arguments.
    fn parse_call_arguments(&mut self) -> Result<Vec<Argument>, String> {
        let mut args = Vec::new();
        let mut seen_named = false;

        if self.check(&TokenKind::RParen) {
            return Ok(args);
        }

        loop {
            let arg_span = self.span();

            // Check if this is a named argument: identifier followed by colon
            // We need to look ahead to distinguish `name: value` from just `name`
            let (name, value) = if let TokenKind::Identifier(ident) = &self.peek().kind {
                let ident = ident.clone();
                // Peek at the next token to see if it's a colon
                if self.current + 1 < self.tokens.len()
                    && matches!(self.tokens[self.current + 1].kind, TokenKind::Colon)
                {
                    // This is a named argument
                    self.advance(); // consume identifier
                    self.advance(); // consume colon
                    seen_named = true;
                    let value = self.expression()?;
                    (Some(ident), value)
                } else {
                    // This is a positional argument
                    if seen_named {
                        return Err(format!(
                            "{}:{}: Positional arguments must come before named arguments",
                            arg_span.line, arg_span.column
                        ));
                    }
                    let value = self.expression()?;
                    (None, value)
                }
            } else {
                // Not an identifier, must be a positional argument
                if seen_named {
                    return Err(format!(
                        "{}:{}: Positional arguments must come before named arguments",
                        arg_span.line, arg_span.column
                    ));
                }
                let value = self.expression()?;
                (None, value)
            };

            args.push(Argument {
                name,
                value,
                span: arg_span,
            });

            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }

        Ok(args)
    }

    fn expect_identifier(&mut self, message: &str) -> Result<String, String> {
        match &self.peek().kind {
            TokenKind::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            _ => Err(format!(
                "{}:{}: {} (found {:?})",
                self.peek().line,
                self.peek().column,
                message,
                self.peek().kind
            )),
        }
    }

    fn expect_type_name(&mut self, message: &str) -> Result<String, String> {
        // Accept both identifiers and type keywords
        match &self.peek().kind {
            TokenKind::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }
            TokenKind::IntType => {
                self.advance();
                Ok("int".to_string())
            }
            TokenKind::FloatType => {
                self.advance();
                Ok("float".to_string())
            }
            TokenKind::BoolType => {
                self.advance();
                Ok("bool".to_string())
            }
            TokenKind::StringType => {
                self.advance();
                Ok("string".to_string())
            }
            TokenKind::CharType => {
                self.advance();
                Ok("char".to_string())
            }
            TokenKind::VoidType => {
                self.advance();
                Ok("void".to_string())
            }
            _ => Err(format!(
                "{}:{}: {} (found {:?})",
                self.peek().line,
                self.peek().column,
                message,
                self.peek().kind
            )),
        }
    }
}

/// Operator precedence levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    None,
    Assignment,   // =, +=, etc.
    NullCoalesce, // ??
    Range,        // .., ..=
    Or,           // ||
    And,          // &&
    BitOr,        // |
    BitXor,       // ^
    BitAnd,       // &
    Equality,     // ==, !=
    Comparison,   // <, <=, >, >=
    Shift,        // <<, >>, >>>
    Term,         // +, -
    Factor,       // *, /, %
    Power,        // **
    Unary,        // !, -, ~
    Cast,         // as, is
    Call,         // (), [], ., ?.
}

impl Precedence {
    fn next(self) -> Self {
        match self {
            Precedence::None => Precedence::Assignment,
            Precedence::Assignment => Precedence::NullCoalesce,
            Precedence::NullCoalesce => Precedence::Range,
            Precedence::Range => Precedence::Or,
            Precedence::Or => Precedence::And,
            Precedence::And => Precedence::BitOr,
            Precedence::BitOr => Precedence::BitXor,
            Precedence::BitXor => Precedence::BitAnd,
            Precedence::BitAnd => Precedence::Equality,
            Precedence::Equality => Precedence::Comparison,
            Precedence::Comparison => Precedence::Shift,
            Precedence::Shift => Precedence::Term,
            Precedence::Term => Precedence::Factor,
            Precedence::Factor => Precedence::Power,
            Precedence::Power => Precedence::Unary,
            Precedence::Unary => Precedence::Cast,
            Precedence::Cast => Precedence::Call,
            Precedence::Call => Precedence::Call,
        }
    }
}

/// Parse tokens into a program AST
pub fn parse(tokens: &[Token]) -> Result<Program, String> {
    let mut parser = Parser::new(tokens.to_vec());
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_expr(source: &str) -> Result<Expression, String> {
        let tokens = tokenize(source)?;
        let mut parser = Parser::new(tokens);
        parser.expression()
    }

    #[test]
    fn test_literals() {
        let expr = parse_expr("42").unwrap();
        assert!(matches!(expr.kind, ExpressionKind::IntLiteral(42)));

        let expr = parse_expr("3.14").unwrap();
        assert!(matches!(expr.kind, ExpressionKind::FloatLiteral(_)));

        let expr = parse_expr("true").unwrap();
        assert!(matches!(expr.kind, ExpressionKind::BoolLiteral(true)));
    }

    #[test]
    fn test_binary_ops() {
        let expr = parse_expr("1 + 2").unwrap();
        assert!(matches!(
            expr.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        let expr = parse_expr("1 + 2 * 3").unwrap();
        // Should parse as 1 + (2 * 3) due to precedence
        if let ExpressionKind::Binary { op, right, .. } = expr.kind {
            assert_eq!(op, BinaryOp::Add);
            assert!(matches!(
                right.kind,
                ExpressionKind::Binary {
                    op: BinaryOp::Mul,
                    ..
                }
            ));
        } else {
            panic!("Expected binary expression");
        }
    }

    #[test]
    fn test_function_call() {
        let expr = parse_expr("foo(1, 2)").unwrap();
        if let ExpressionKind::Call { args, .. } = expr.kind {
            assert_eq!(args.len(), 2);
        } else {
            panic!("Expected call expression");
        }
    }

    #[test]
    fn test_named_arguments() {
        // Test parsing: foo(x: 1, y: 2)
        let expr = parse_expr("foo(x: 1, y: 2)").unwrap();
        if let ExpressionKind::Call { args, .. } = expr.kind {
            assert_eq!(args.len(), 2);
            assert_eq!(args[0].name, Some("x".to_string()));
            assert_eq!(args[1].name, Some("y".to_string()));
        } else {
            panic!("Expected call expression");
        }
    }

    #[test]
    fn test_mixed_positional_and_named_arguments() {
        // Test parsing: foo(1, y: 2, z: 3)
        let expr = parse_expr("foo(1, y: 2, z: 3)").unwrap();
        if let ExpressionKind::Call { args, .. } = expr.kind {
            assert_eq!(args.len(), 3);
            assert_eq!(args[0].name, None); // positional
            assert_eq!(args[1].name, Some("y".to_string()));
            assert_eq!(args[2].name, Some("z".to_string()));
        } else {
            panic!("Expected call expression");
        }
    }

    #[test]
    fn test_positional_after_named_error() {
        // Positional arguments after named should error
        let result = parse_expr("foo(x: 1, 2)");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Positional arguments must come before named arguments"));
    }

    #[test]
    fn test_array_type_annotation() {
        // Test parsing: let arr: [int] = [1, 2, 3]
        let source = "let arr: [int] = [1, 2, 3]";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);

        if let StatementKind::VarDecl {
            pattern, type_ann, ..
        } = &program.statements[0].kind
        {
            if let PatternKind::Variable(name) = &pattern.kind {
                assert_eq!(name, "arr");
            } else {
                panic!("Expected variable pattern");
            }
            assert!(type_ann.is_some());
            let type_expr = type_ann.as_ref().unwrap();
            assert!(matches!(type_expr.kind, TypeExprKind::Array(_)));
        } else {
            panic!("Expected variable declaration");
        }
    }

    #[test]
    fn test_nested_array_type() {
        // Test parsing nested arrays: let matrix: [[int]] = [[1, 2], [3, 4]]
        let source = "let matrix: [[int]] = [[1, 2]]";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::VarDecl { type_ann, .. } = &program.statements[0].kind {
            let type_expr = type_ann.as_ref().unwrap();
            if let TypeExprKind::Array(inner) = &type_expr.kind {
                assert!(matches!(inner.kind, TypeExprKind::Array(_)));
            } else {
                panic!("Expected array type");
            }
        } else {
            panic!("Expected variable declaration");
        }
    }

    #[test]
    fn test_enum_variant_expression() {
        // Test parsing: Color::Red
        let expr = parse_expr("Color::Red").unwrap();
        if let ExpressionKind::EnumVariant {
            enum_name,
            variant_name,
        } = expr.kind
        {
            assert_eq!(enum_name, "Color");
            assert_eq!(variant_name, "Red");
        } else {
            panic!("Expected enum variant expression");
        }
    }

    #[test]
    fn test_enum_variant_in_match() {
        // Test parsing enum variant in match expression
        let source = r#"
            match color {
                Color::Red => 1
                Color::Blue => 2
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let mut parser = Parser::new(tokens);
        let expr = parser.expression().unwrap();

        if let ExpressionKind::Match { arms, .. } = expr.kind {
            assert_eq!(arms.len(), 2);
            // Check first arm pattern is Color::Red (uses Constructor pattern)
            if let PatternKind::Constructor { name, .. } = &arms[0].pattern.kind {
                assert_eq!(name, "Color::Red");
            } else {
                panic!("Expected constructor pattern for enum variant");
            }
        } else {
            panic!("Expected match expression");
        }
    }

    #[test]
    fn test_import_statement() {
        let source = "import std.fs";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::Import { path, items } = &program.statements[0].kind {
            assert_eq!(path, &vec!["std".to_string(), "fs".to_string()]);
            assert!(items.is_none()); // Wildcard import
        } else {
            panic!("Expected import statement");
        }
    }

    #[test]
    fn test_selective_import() {
        let source = "import std.io.{read, write}";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::Import { path, items } = &program.statements[0].kind {
            assert_eq!(path, &vec!["std".to_string(), "io".to_string()]);
            let items = items.as_ref().unwrap();
            assert!(items.contains(&"read".to_string()));
            assert!(items.contains(&"write".to_string()));
        } else {
            panic!("Expected import statement");
        }
    }

    #[test]
    fn test_function_with_array_param() {
        let source = "fn sum(arr: [int]) -> int { return 0 }";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::FnDecl { params, .. } = &program.statements[0].kind {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "arr");
            assert!(matches!(params[0].type_ann.kind, TypeExprKind::Array(_)));
        } else {
            panic!("Expected function declaration");
        }
    }

    #[test]
    fn test_unary_operators() {
        // Test prefix operators
        let expr = parse_expr("-42").unwrap();
        assert!(matches!(
            expr.kind,
            ExpressionKind::Unary {
                op: UnaryOp::Neg,
                ..
            }
        ));

        let expr = parse_expr("!true").unwrap();
        assert!(matches!(
            expr.kind,
            ExpressionKind::Unary {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn test_compound_assignment() {
        let source = "x += 1";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::Expression(expr) = &program.statements[0].kind {
            if let ExpressionKind::CompoundAssign { op, .. } = &expr.kind {
                assert_eq!(*op, BinaryOp::Add);
            } else {
                panic!("Expected compound assignment");
            }
        } else {
            panic!("Expected expression statement");
        }
    }

    #[test]
    fn test_increment_decrement() {
        // Prefix increment
        let expr = parse_expr("++x").unwrap();
        assert!(matches!(
            expr.kind,
            ExpressionKind::Unary {
                op: UnaryOp::PreInc,
                ..
            }
        ));

        // Prefix decrement
        let expr = parse_expr("--x").unwrap();
        assert!(matches!(
            expr.kind,
            ExpressionKind::Unary {
                op: UnaryOp::PreDec,
                ..
            }
        ));
    }

    // ========================================================================
    // Trait Declaration Tests
    // ========================================================================

    #[test]
    fn test_simple_trait() {
        let source = "trait Clone { }";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);

        if let StatementKind::TraitDecl { name, methods, .. } = &program.statements[0].kind {
            assert_eq!(name, "Clone");
            assert!(methods.is_empty());
        } else {
            panic!("Expected TraitDecl");
        }
    }

    #[test]
    fn test_trait_with_method() {
        let source = r#"
            trait Clone {
                fn clone(self) -> Self
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::TraitDecl { name, methods, .. } = &program.statements[0].kind {
            assert_eq!(name, "Clone");
            assert_eq!(methods.len(), 1);
            assert_eq!(methods[0].name, "clone");
            assert!(methods[0].has_self);
            assert!(methods[0].default_impl.is_none());
        } else {
            panic!("Expected TraitDecl");
        }
    }

    #[test]
    fn test_trait_with_default_method() {
        let source = r#"
            trait Eq {
                fn eq(self, other: Self) -> bool

                fn ne(self, other: Self) -> bool {
                    return !self.eq(other)
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::TraitDecl { name, methods, .. } = &program.statements[0].kind {
            assert_eq!(name, "Eq");
            assert_eq!(methods.len(), 2);
            assert_eq!(methods[0].name, "eq");
            assert!(methods[0].default_impl.is_none());
            assert_eq!(methods[1].name, "ne");
            assert!(methods[1].default_impl.is_some());
        } else {
            panic!("Expected TraitDecl");
        }
    }

    #[test]
    fn test_trait_with_type_params() {
        let source = r#"
            trait Into<T> {
                fn into(self) -> T
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::TraitDecl {
            name,
            type_params,
            methods,
            ..
        } = &program.statements[0].kind
        {
            assert_eq!(name, "Into");
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "T");
            assert_eq!(methods.len(), 1);
        } else {
            panic!("Expected TraitDecl");
        }
    }

    #[test]
    fn test_public_trait() {
        let source = "pub trait Serializable { }";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::TraitDecl {
            name, is_public, ..
        } = &program.statements[0].kind
        {
            assert_eq!(name, "Serializable");
            assert!(is_public);
        } else {
            panic!("Expected TraitDecl");
        }
    }

    // ========================================================================
    // Impl Block Tests
    // ========================================================================

    #[test]
    fn test_simple_impl() {
        let source = "impl Point { }";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);

        if let StatementKind::ImplDecl {
            type_name,
            trait_name,
            methods,
            ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "Point");
            assert!(trait_name.is_none());
            assert!(methods.is_empty());
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_impl_with_method() {
        let source = r#"
            impl Point {
                fn distance(self, other: Point) -> float {
                    return 0.0
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl {
            type_name, methods, ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "Point");
            assert_eq!(methods.len(), 1);
            if let StatementKind::FnDecl { name, .. } = &methods[0].kind {
                assert_eq!(name, "distance");
            } else {
                panic!("Expected function in impl");
            }
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_impl_with_static_method() {
        let source = r#"
            impl Point {
                fn origin() -> Point {
                    return Point { x: 0, y: 0 }
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl {
            type_name, methods, ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "Point");
            assert_eq!(methods.len(), 1);
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_generic_impl() {
        let source = r#"
            impl<T> List<T> {
                fn first(self) -> T {
                    return self[0]
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl {
            type_name,
            type_params,
            ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "List");
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "T");
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_trait_impl() {
        let source = r#"
            impl Clone for Point {
                fn clone(self) -> Point {
                    return Point { x: self.x, y: self.y }
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl {
            type_name,
            trait_name,
            methods,
            ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "Point");
            assert_eq!(trait_name.as_deref(), Some("Clone"));
            assert_eq!(methods.len(), 1);
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_generic_trait_impl() {
        let source = r#"
            impl<T: Clone> Clone for List<T> {
                fn clone(self) -> List<T> {
                    return self.map(|x| x.clone())
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl {
            type_name,
            trait_name,
            type_params,
            ..
        } = &program.statements[0].kind
        {
            assert_eq!(type_name, "List");
            assert_eq!(trait_name.as_deref(), Some("Clone"));
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name, "T");
            assert!(type_params[0].bounds.contains(&"Clone".to_string()));
        } else {
            panic!("Expected ImplDecl");
        }
    }

    // ========================================================================
    // Self Receiver Tests
    // ========================================================================

    #[test]
    fn test_self_param_in_function() {
        let source = r#"
            impl Counter {
                fn get(self) -> int {
                    return self.value
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl { methods, .. } = &program.statements[0].kind {
            if let StatementKind::FnDecl { params, .. } = &methods[0].kind {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "self");
            } else {
                panic!("Expected function");
            }
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_self_mut_param() {
        let source = r#"
            impl Counter {
                fn increment(self mut) {
                    self.value += 1
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl { methods, .. } = &program.statements[0].kind {
            if let StatementKind::FnDecl { params, .. } = &methods[0].kind {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "self");
                // Note: mutability should be tracked somehow
            } else {
                panic!("Expected function");
            }
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_impl_for_builtin_type() {
        let source = r#"
            impl string {
                fn is_empty(self) -> bool {
                    return self.len() == 0
                }
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::ImplDecl { type_name, .. } = &program.statements[0].kind {
            assert_eq!(type_name, "string");
        } else {
            panic!("Expected ImplDecl");
        }
    }

    #[test]
    fn test_try_operator() {
        let source = r#"
            fn test() {
                let x = some_function()?
                let y = obj.method()?
            }
        "#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();

        if let StatementKind::FnDecl { body, .. } = &program.statements[0].kind {
            // First statement: let x = some_function()?
            if let StatementKind::VarDecl {
                initializer: Some(init),
                ..
            } = &body.statements[0].kind
            {
                if let ExpressionKind::Try(inner) = &init.kind {
                    assert!(matches!(&inner.kind, ExpressionKind::Call { .. }));
                } else {
                    panic!("Expected Try expression");
                }
            } else {
                panic!("Expected VarDecl with initializer");
            }

            // Second statement: let y = obj.method()?
            if let StatementKind::VarDecl {
                initializer: Some(init),
                ..
            } = &body.statements[1].kind
            {
                if let ExpressionKind::Try(inner) = &init.kind {
                    // obj.method() is parsed as Call with FieldAccess callee
                    assert!(matches!(&inner.kind, ExpressionKind::Call { .. }));
                } else {
                    panic!("Expected Try expression");
                }
            } else {
                panic!("Expected VarDecl with initializer");
            }
        } else {
            panic!("Expected FnDecl");
        }
    }
}
