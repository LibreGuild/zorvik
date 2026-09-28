//! Which operation a GraphQL document runs, without a full parser: a subscription is sent over
//! WebSocket or SSE (see [`GraphqlTransport`](crate::GraphqlTransport)), queries and mutations
//! over plain HTTP.

/// The kind of a GraphQL operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Query,
    Mutation,
    Subscription,
}

/// The operations defined in `document`: their type and name (`None` when anonymous).
/// Fragments are skipped. Only the definitions' first words are read, so a document with
/// mistakes inside its selection sets still says which operations it has.
pub fn operations(document: &str) -> Vec<(OperationType, Option<String>)> {
    enum State {
        /// Expecting a definition.
        Start,
        /// After `query`, `mutation` or `subscription`: the name may follow.
        Keyword(OperationType),
        /// Variables and directives, up to the selection set.
        Header,
        /// A fragment (or anything else): skipped up to its selection set.
        Other,
    }
    let mut out = Vec::new();
    let mut lexer = Lexer { text: document, at: 0 };
    let mut state = State::Start;
    while let Some(token) = lexer.next() {
        state = match (state, token) {
            // `{ … }` alone is an anonymous query.
            (State::Start, Token::Open) => {
                out.push((OperationType::Query, None));
                lexer.skip_block();
                State::Start
            }
            (State::Start, Token::Name(word)) => match word {
                "query" => State::Keyword(OperationType::Query),
                "mutation" => State::Keyword(OperationType::Mutation),
                "subscription" => State::Keyword(OperationType::Subscription),
                _ => State::Other,
            },
            (State::Start, _) => State::Start,
            (State::Keyword(kind), Token::Name(name)) => {
                out.push((kind, Some(name.to_string())));
                State::Header
            }
            (State::Keyword(kind), Token::Open) => {
                out.push((kind, None));
                lexer.skip_block();
                State::Start
            }
            (State::Keyword(kind), _) => {
                out.push((kind, None));
                State::Header
            }
            (State::Header | State::Other, Token::Open) => {
                lexer.skip_block();
                State::Start
            }
            (state @ (State::Header | State::Other), _) => state,
        };
    }
    if let State::Keyword(kind) = state {
        out.push((kind, None));
    }
    out
}

/// The type of the operation that runs: the one named `operation_name`, or the only one.
pub fn operation_type(document: &str, operation_name: Option<&str>) -> Option<OperationType> {
    let ops = operations(document);
    match operation_name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => ops.iter().find(|(_, n)| n.as_deref() == Some(name)).map(|(kind, _)| *kind),
        None if ops.len() == 1 => Some(ops[0].0),
        None => None,
    }
}

enum Token<'a> {
    Name(&'a str),
    Open,
    Close,
    /// Anything else: punctuation, numbers, strings, a whole `( … )`.
    Other,
}

struct Lexer<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.rest().chars().next()?;
        self.at += c.len_utf8();
        Some(c)
    }

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let start = self.at;
            let c = self.bump()?;
            return Some(match c {
                c if c.is_whitespace() || c == ',' || c == '\u{feff}' => continue,
                '#' => {
                    self.skip_comment();
                    continue;
                }
                '"' => {
                    self.skip_string();
                    Token::Other
                }
                '{' => Token::Open,
                '}' => Token::Close,
                '(' => {
                    self.skip_parens();
                    Token::Other
                }
                c if c == '_' || c.is_ascii_alphabetic() => {
                    let len =
                        self.rest().find(|c: char| c != '_' && !c.is_ascii_alphanumeric()).unwrap_or(self.rest().len());
                    self.at += len;
                    Token::Name(&self.text[start..self.at])
                }
                _ => Token::Other,
            });
        }
    }

    fn skip_comment(&mut self) {
        let len = self.rest().find(['\n', '\r']).unwrap_or(self.rest().len());
        self.at += len;
    }

    /// After an opening quote: the rest of a string or of a `"""block string"""`.
    fn skip_string(&mut self) {
        if let Some(block) = self.rest().strip_prefix("\"\"") {
            self.at += 2;
            // `\"""` doesn't end a block string.
            let mut from = 0;
            loop {
                match block[from..].find("\"\"\"") {
                    Some(i) if block[..from + i].ends_with('\\') => from += i + 3,
                    Some(i) => {
                        self.at += from + i + 3;
                        return;
                    }
                    None => {
                        self.at = self.text.len();
                        return;
                    }
                }
            }
        }
        while let Some(c) = self.bump() {
            match c {
                '\\' => {
                    self.bump();
                }
                '"' | '\n' | '\r' => return,
                _ => {}
            }
        }
    }

    /// After `{`: up to the brace that closes it.
    fn skip_block(&mut self) {
        let mut depth = 1;
        while depth > 0 {
            match self.next() {
                Some(Token::Open) => depth += 1,
                Some(Token::Close) => depth -= 1,
                Some(_) => {}
                None => return,
            }
        }
    }

    /// After `(`: up to the parenthesis that closes it.
    fn skip_parens(&mut self) {
        let mut depth = 1;
        while depth > 0 {
            match self.bump() {
                Some('(') => depth += 1,
                Some(')') => depth -= 1,
                Some('"') => self.skip_string(),
                Some('#') => self.skip_comment(),
                Some(_) => {}
                None => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OperationType::{Mutation, Query, Subscription};
    use super::*;

    fn names(doc: &str) -> Vec<(OperationType, Option<String>)> {
        operations(doc)
    }

    fn expect(list: &[(OperationType, Option<&str>)]) -> Vec<(OperationType, Option<String>)> {
        list.iter().map(|(k, n)| (*k, n.map(String::from))).collect()
    }

    #[test]
    fn finds_operations_and_their_names() {
        assert_eq!(names("{ ping }"), expect(&[(Query, None)]));
        assert_eq!(names("query { a }"), expect(&[(Query, None)]));
        assert_eq!(names("subscription OnMessage { message { id } }"), expect(&[(Subscription, Some("OnMessage"))]));
        assert_eq!(
            names(
                r#"# a comment with query { x }
                query Get($id: ID = "a { b") @cached(ttl: {s: 1}) { user(id: $id) { ...F } }
                fragment F on User { id name(format: "} subscription X {") }
                mutation Save { save(input: """block "quoted" \""" subscription {""") }
                subscription($room: ID!) { messages(room: $room) }"#
            ),
            expect(&[(Query, Some("Get")), (Mutation, Some("Save")), (Subscription, None)])
        );
        // Unfinished documents (while typing).
        assert_eq!(names("subscription"), expect(&[(Subscription, None)]));
        assert_eq!(names("subscription Live { ticks { "), expect(&[(Subscription, Some("Live"))]));
        assert!(names("").is_empty());
    }

    #[test]
    fn the_operation_that_runs() {
        let doc = "query A { a } subscription B { b }";
        assert_eq!(operation_type(doc, Some("B")), Some(Subscription));
        assert_eq!(operation_type(doc, Some(" A ")), Some(Query));
        assert_eq!(operation_type(doc, None), None, "two operations and no name: the server says which is missing");
        assert_eq!(operation_type(doc, Some("C")), None);
        assert_eq!(operation_type("subscription { b }", None), Some(Subscription));
        assert_eq!(operation_type("subscription { b }", Some("")), Some(Subscription));
    }
}
