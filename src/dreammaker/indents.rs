//! The indentation processor.
use std::collections::VecDeque;

use super::{DMError, Location, HasLocation, Context};
use super::lexer::{LocatedToken, Token, Punctuation};

/// Eliminates blank lines, parses and validates indentation, braces, and semicolons.
///
/// After processing, no Newline, Tab, or Space tokens remain.
pub struct IndentProcessor<'ctx, I> {
    context: &'ctx Context,
    inner: I,

    last_input_loc: Location,
    output: VecDeque<Token>,

    // If we're indented, the number of spaces per indent and the number of indents.
    current: Option<(usize, usize)>,
    // The number of spaces/tabs accumulated on the current line. None when not at line head.
    current_spaces: Option<usize>,
    parentheses: usize,
    eof_yielded: bool,
}

impl<'ctx, I> HasLocation for IndentProcessor<'ctx, I> {
    fn location(&self) -> Location {
        self.last_input_loc
    }
}

impl<'ctx, I> IndentProcessor<'ctx, I> where
    I: Iterator<Item=Result<LocatedToken, DMError>>
{
    pub fn new<J: IntoIterator<Item=Result<LocatedToken, DMError>, IntoIter=I>>(context: &'ctx Context, inner: J) -> Self {
        IndentProcessor {
            context,
            inner: inner.into_iter(),
            last_input_loc: Location::default(),
            output: VecDeque::new(),
            current: None,
            current_spaces: None,
            parentheses: 0,
            eof_yielded: false,
        }
    }

    #[inline]
    fn inner_next(&mut self) -> Option<Result<LocatedToken, DMError>> {
        self.inner.next()
    }

    fn real_next(&mut self, read: Token) {
        // handle whitespace
        match read {
            Token::Punct(Punctuation::Newline) => {
                if self.parentheses == 0 {
                    self.current_spaces = Some(0);
                }
                // semicolons are placed by the first token on the next line
                return;
            }
            Token::Punct(Punctuation::Tab) |
            Token::Punct(Punctuation::Space) => {
                if let Some(spaces) = self.current_spaces.as_mut() {
                    *spaces += 1;
                }
                return;
            }
            _ => {}
        }

        // handle pre-existing braces
        match read {
            Token::Punct(Punctuation::LBrace) => self.current_spaces = None,
            Token::Punct(Punctuation::RBrace) => {
                self.current_spaces = None;
                if self.parentheses == 0 {
                    self.output.push_back(Token::Punct(Punctuation::Semicolon));
                }
            }
            _ => {}
        }

        // handle indentation
        if let Some(spaces) = self.current_spaces.take() {
            let (indents, new_indents);
            match self.current {
                None => {
                    indents = 0;
                    if spaces == 0 {
                        new_indents = 0;
                        self.current = None;
                    } else {
                        new_indents = 1;
                        self.current = Some((spaces, 1));
                    }
                }
                Some((spaces_per_indent, indents_)) => {
                    indents = indents_;
                    if spaces == 0 {
                        self.current = None;
                        new_indents = 0;
                    } else {
                        if spaces % spaces_per_indent != 0 {
                            // Register the error, but cross our fingers and
                            // hope that truncating division will approximate
                            // a sane situation.
                            self.context.register_error(self.error(format!(
                                "inconsistent indentation: {} % {} != 0", spaces, spaces_per_indent)));
                        }
                        new_indents = spaces / spaces_per_indent;
                        self.current = Some((spaces_per_indent, new_indents));
                    }
                }
            }

            if indents + 1 == new_indents {
                // single indent
                self.output.push_back(Token::Punct(Punctuation::LBrace));
            } else if indents < new_indents {
                // multiple indent is an error, register it but let it work
                self.context.register_error(self.error(format!(
                    "inconsistent multiple indentation: {} > 1", new_indents - indents)));
                for _ in indents..new_indents {
                    self.output.push_back(Token::Punct(Punctuation::LBrace));
                }
            } else if indents == new_indents + 1 {
                // single unindent
                self.output.push_back(Token::Punct(Punctuation::Semicolon));
                self.output.push_back(Token::Punct(Punctuation::RBrace));
            } else if indents > new_indents {
                // multiple unindent
                self.output.push_back(Token::Punct(Punctuation::Semicolon));
                for _ in new_indents..indents {
                    self.output.push_back(Token::Punct(Punctuation::RBrace));
                }
            } else {
                // same indent as before
                self.output.push_back(Token::Punct(Punctuation::Semicolon));
            }
        }

        // handle non-whitespace
        match read {
            Token::Punct(Punctuation::LBrace) => {
                self.current = match self.current {
                    None => Some((1, 1)),
                    Some((x, y)) => Some((x, y + 1)),
                };
            }
            Token::Punct(Punctuation::RBrace) => {
                self.current = match self.current {
                    None => {
                        self.context.register_error(self.error("unmatched right brace"));
                        None
                    }
                    Some((_, 1)) => None,
                    Some((x, y)) => Some((x, y - 1)),
                };
            }
            Token::Punct(Punctuation::LParen) => {
                self.parentheses += 1;
            }
            Token::Punct(Punctuation::RParen) => {
                self.parentheses -= 1;
            }
            _ => {}
        }

        self.output.push_back(read);
    }
}

impl<'ctx, I> Iterator for IndentProcessor<'ctx, I> where
    I: Iterator<Item=Result<LocatedToken, DMError>>
{
    type Item = Result<LocatedToken, DMError>;

    fn next(&mut self) -> Option<Result<LocatedToken, DMError>> {
        loop {
            if let Some(token) = self.output.pop_front() {
                return Some(Ok(LocatedToken {
                    location: self.last_input_loc,
                    token: token,
                }));
            }

            if let Some(tok) = self.inner_next() {
                let tok = try_iter!(tok);
                self.last_input_loc = tok.location;
                self.real_next(tok.token);
            } else if self.eof_yielded {
                return None;
            } else {
                self.output.push_back(Token::Punct(Punctuation::Semicolon));
                if let Some((_, indents)) = self.current {
                    for _ in 0..indents {
                        self.output.push_back(Token::Punct(Punctuation::RBrace));
                    }
                }
                self.current = None;
                self.eof_yielded = true;
            }
        }
    }
}
