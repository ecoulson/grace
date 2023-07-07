extern crate rocket;

use crate::database;
use crate::indexer::{Indexer, Link};
use mysql::{params, Transaction, TxOpts};
use mysql::{prelude::Queryable, PooledConn};
use std::collections::HashMap;
use std::iter::Peekable;
use std::time::Duration;
use ureq::AgentBuilder;

#[derive(Debug)]
pub struct HTMLNode {
    tag: String,
    attributes: HashMap<String, Option<String>>,
    children: Vec<HTMLNode>,
}

#[derive(Debug)]
pub struct Lexer {
    position: usize,
    characters: Vec<char>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum Token {
    TagStart,
    TagEnd,
    TagCloseStart,
    TagCloseEnd,
    Whitespace,
    Comment,
    StringLiteral(String),
    Identifier(String),
    Equal,
}

pub enum GraceError {
    MySQL(mysql::Error),
    UReq(ureq::Error),
    IO(std::io::Error),
}

impl From<mysql::Error> for GraceError {
    fn from(value: mysql::Error) -> Self {
        Self::MySQL(value)
    }
}

impl From<ureq::Error> for GraceError {
    fn from(value: ureq::Error) -> Self {
        Self::UReq(value)
    }
}

impl From<std::io::Error> for GraceError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

pub fn crawl() -> Result<(), GraceError> {
    let mut connection = database::connect_to_database().expect("Failed to connect to PlanetScale");
    let mut indexers = load_indexers(&mut connection)?;

    if indexers.is_empty() {
        indexers.push(create_default_indexer(&mut connection)?);
    }

    let indexer = indexers
        .first_mut()
        .expect("At least one indexer should exist");

    while let Some(link) = indexer.url_queue.pop() {
        let agent = AgentBuilder::new()
            .timeout_read(Duration::from_secs(5))
            .build();
        let html = agent.get(link.url.as_str()).call()?.into_string()?; // Change to reader
        let mut lexer = Lexer {
            position: 0,
            characters: html.chars().collect(),
        };
        let tokens = tokenize(&mut lexer);
        let dom = parse(&mut tokens.iter().peekable());

        dbg!(dom);

        println!("Completed update");
    }

    Ok(())
}

fn tokenize(lexer: &mut Lexer) -> Vec<Token> {
    let mut tokens = vec![];

    while lexer.position < lexer.characters.len() {
        let possible_tokens = vec![
            lex_comment(lexer),
            lex_whitespace(lexer),
            lex_identifier(lexer),
            lex_string_literal(lexer),
            lex_symbols(lexer),
        ];

        let (token, position) = possible_tokens
            .iter()
            .reduce(
                |best, current| {
                    if best.1 < current.1 {
                        current
                    } else {
                        best
                    }
                },
            )
            .expect("Should reduce to a token");

        tokens.push(token.clone());

        lexer.position = position.clone();
    }

    tokens
}

fn lex_comment(lexer: &mut Lexer) -> (Token, usize) {
    let mut current_position = lexer.position;

    for ch in "<!--".chars() {
        if current_position >= lexer.characters.len() || lexer.characters[current_position] != ch {
            return (Token::Comment, 0);
        }

        current_position += 1;
    }

    while current_position < lexer.characters.len() && !is_closing_comment(lexer, current_position)
    {
        current_position += 1;
    }

    current_position += 3; // length of -->

    return (Token::Comment, current_position);
}

fn is_closing_comment(lexer: &mut Lexer, current_position: usize) -> bool {
    let mut current_position = current_position;

    for ch in "-->".chars() {
        if current_position >= lexer.characters.len() || lexer.characters[current_position] != ch {
            return false;
        }

        current_position += 1;
    }

    true
}

fn lex_whitespace(lexer: &mut Lexer) -> (Token, usize) {
    let mut current_position = lexer.position;

    while current_position < lexer.characters.len()
        && lexer.characters[current_position].is_whitespace()
    {
        current_position += 1;
    }

    (Token::Whitespace, current_position)
}

fn lex_identifier(lexer: &mut Lexer) -> (Token, usize) {
    let mut identifier = String::new();
    let mut current_position = lexer.position;

    if current_position < lexer.characters.len()
        && lexer.characters[current_position].is_alphabetic()
    {
        identifier.push(lexer.characters[current_position]);
        current_position += 1;
    } else {
        return (Token::Identifier(identifier), 0);
    }

    while current_position < lexer.characters.len()
        && lexer.characters[current_position].is_alphanumeric()
    {
        identifier.push(lexer.characters[current_position]);
        current_position += 1;
    }

    (Token::Identifier(identifier), current_position)
}

fn lex_string_literal(lexer: &mut Lexer) -> (Token, usize) {
    let mut current_position = lexer.position;
    let mut string_literal = String::new();

    if current_position >= lexer.characters.len() {
        return (Token::StringLiteral(string_literal), 0);
    }

    let quote = match lexer.characters[current_position] {
        '\'' => Some('\''),
        '"' => Some('"'),
        _ => None,
    };

    if quote.is_none() {
        return (Token::StringLiteral(string_literal), 0);
    }

    let quote = quote.expect("Should be a quote char");

    current_position += 1;

    while current_position < lexer.characters.len() && lexer.characters[current_position] != quote {
        string_literal.push(lexer.characters[current_position]);
        current_position += 1;
    }

    current_position += 1;

    (Token::StringLiteral(string_literal), current_position)
}

fn lex_symbols(lexer: &mut Lexer) -> (Token, usize) {
    if lexer.position + 1 < lexer.characters.len() {
        let long_symbol = match lexer.characters[lexer.position..lexer.position + 2] {
            ['<', '/'] => Some((Token::TagCloseEnd, lexer.position + 2)),
            ['/', '>'] => Some((Token::TagCloseStart, lexer.position + 2)),
            _ => None,
        };

        if long_symbol.is_some() {
            return long_symbol.expect("Should be a long symbol");
        }
    }

    if lexer.position >= lexer.characters.len() {
        return (Token::Whitespace, 0);
    }

    match lexer.characters[lexer.position] {
        '=' => (Token::Equal, lexer.position + 1),
        '<' => (Token::TagStart, lexer.position + 1),
        '>' => (Token::TagEnd, lexer.position + 1),
        _ => (Token::Whitespace, 0),
    }
}

fn parse(tokens: &mut Peekable<std::slice::Iter<'_, Token>>) -> HTMLNode {
    if let Some(Token::TagStart) = tokens.peek() {
        tokens.next();
    } else {
        panic!("Expected tag start");
    }

    let tag = match tokens.next() {
        Some(Token::Identifier(tag)) => tag.clone(),
        _ => String::new(),
    };

    while let Some(Token::Whitespace) = tokens.peek() {
        tokens.next();
    }

    let attributes = parse_attributes(tokens);

    while let Some(Token::Whitespace) = tokens.peek() {
        tokens.next();
    }

    if let Some(Token::TagCloseStart) = tokens.peek() {
        tokens.next();
        return HTMLNode {
            tag: tag.clone(),
            attributes,
            children: vec![],
        };
    }

    if let Some(Token::TagEnd) = tokens.peek() {
        tokens.next();
    } else {
        panic!("Exepected tag end");
    }

    while let Some(Token::Whitespace) = tokens.peek() {
        tokens.next();
    }

    let mut children = vec![];

    while let Some(Token::TagStart) = tokens.peek() {
        children.push(parse(tokens));

        while let Some(Token::Whitespace) = tokens.peek() {
            tokens.next();
        }
    }

    if let Some(Token::TagCloseEnd) = tokens.peek() {
        tokens.next();
    } else {
        panic!("Expected tag close");
    }

    if let Some(Token::Identifier(closing_tag_name)) = tokens.next() {
        if closing_tag_name != &tag {
            panic!("Invalid html");
        }
    }

    if let Some(Token::TagEnd) = tokens.peek() {
        tokens.next();
    } else {
        panic!("Expected tag end");
    }

    HTMLNode {
        tag: tag.clone(),
        attributes,
        children,
    }
}

fn parse_attributes(
    tokens: &mut Peekable<std::slice::Iter<'_, Token>>,
) -> HashMap<String, Option<String>> {
    let mut attributes = HashMap::new();

    while let Some(Token::Identifier(attribute_name)) = tokens.peek() {
        tokens.next();

        while let Some(Token::Whitespace) = tokens.peek() {
            tokens.next();
        }

        if let Some(Token::Equal) = tokens.peek() {
            tokens.next();
            if let Some(Token::StringLiteral(value)) = tokens.next() {
                attributes.insert(attribute_name.clone(), Some(value.clone()));
            } else {
                panic!("Illegal attribute");
            }
        } else {
            attributes.insert(attribute_name.clone(), None);
        }

        while let Some(Token::Whitespace) = tokens.peek() {
            tokens.next();
        }
    }

    attributes
}

fn load_indexers(connection: &mut PooledConn) -> Result<Vec<Indexer>, mysql::Error> {
    let mut transaction = connection.start_transaction(TxOpts::default())?;

    let ids: Vec<u64> = transaction.query("SELECT id FROM indexers")?;
    let indexers = ids
        .iter()
        .map(|id| Indexer {
            id: *id,
            url_queue: load_indexer_queue(&mut transaction, *id)
                .expect("Failed to load indexer queue"),
        })
        .collect();

    transaction.commit()?;

    Ok(indexers)
}

fn create_default_indexer(connection: &mut PooledConn) -> Result<Indexer, mysql::Error> {
    let mut transaction = connection.start_transaction(TxOpts::default())?;

    transaction.query_drop("INSERT INTO indexers (id) VALUES (NULL)")?;
    let id = transaction
        .last_insert_id()
        .expect("Transaction should have commited an indexer");
    let default_url = String::from("https://neovim.io/doc/user/api.html#api");
    transaction.exec_drop(
        "INSERT INTO links (id, url) VALUES (NULL, :url)",
        params! {
            "url" => &default_url
        },
    )?;
    let link_id = transaction
        .last_insert_id()
        .expect("Transaction should have commited a link");
    transaction.exec_drop(
        "INSERT INTO queue_links (id, link_id) VALUES (:id, :link_id)",
        params! {
            id, link_id
        },
    )?;
    transaction.commit()?;

    Ok(Indexer {
        id,
        url_queue: vec![Link {
            text: None,
            url: default_url,
            id: link_id,
        }],
    })
}

fn load_indexer_queue(
    transaction: &mut Transaction,
    indexer_id: u64,
) -> Result<Vec<Link>, mysql::Error> {
    let link_ids: Vec<u64> = transaction.exec(
        "SELECT link_id FROM queue_links WHERE id = :indexer_id",
        params! {
            indexer_id
        },
    )?;
    let urls = transaction.exec_map(
        "SELECT id, url FROM links WHERE id IN (:link_ids)",
        params! {
            "link_ids" => link_ids.iter()
                                  .map(|x| x.to_string())
                                  .collect::<Vec<String>>()
                                  .join(", "),
        },
        |(id, url)| Link {
            id,
            url,
            text: None,
        },
    )?;

    Ok(urls)
}

#[cfg(test)]
mod tests {
    use crate::crawler::{parse, tokenize, Lexer};

    #[test]
    fn html_test() {
        let html = "<html><button class=\"myballs\"/></html>";
        let tokens = tokenize(&mut Lexer {
            position: 0,
            characters: html.chars().collect(),
        });
        let dom = parse(&mut tokens.iter().peekable());

        dbg!(dom);

        panic!();
    }
}
