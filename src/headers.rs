//! This module contains the implementation of header serialization and deserialization
//!
//! While the header ringbuffer is just like the data ringbuffer, there is "convention" that
//! it contains key/value pairs of ASCII plaintext. Nothing about the C abstraction guarantees that,
//! so we still have a public API for dealing with the raw bytes. However, this module will introduce some
//! methods that implement the serde to and from these bytes and automaticlly read and write. These methods
//! are of course falliable, because who knows what people put on the header buffer.
//!
//! ## Parsing
//!
//! From the [specification](http://psrdada.sourceforge.net/manuals/Specification.pdf):
//!
//! > Data attributes are stored as keyword-value pairs, each separated by a new line.
//!
//! The specification does not mention how these key value pairs are separated, and we will interpret new line
//! as `\n` or `\r\n`. Missing from the specification is how the key/value pairs are separated. We will
//! use some examples to guess that they are separated by any amount of whitespace (tabs or spaces, not newlines or carriage returns).
//! Also, in some examples, we see `#` denoting comments in headers, in which case we will ignore when parsing.
//! We will also ignore empty lines.
//!
//! Formally, we can write this grammar in "EBNF" as:
//! ```ebnf
//! Header       := (Pair <Newline> | <Newline>)+  Pair? EOF?
//! Pair         := Key <Whitespace> Value <Whitespace?> <Comment?>
//!
//! Key          := Token
//! Value        := Token
//!
//! <Token>      := #"[^\r\n\t\f\v#\0]+"
//! <Comment>    := "#" #"[^\n\r\0]*"
//! <Whitespace> := #"[^\S\r\n\0]+"
//! <Newline>    := "\n" | "\r\n"
//! <EOF>        := "\0"
//! ```
//!
//! Where #"..." are PCRE regular expressions.
//! This grammar will work as is with the [instaparse](https://github.com/Engelberg/instaparse) library from Clojure.
//!
//! ## Serializing
//!
//! Going from a `HashMap<String,String>`, we will print keys as is, separated by a single space with newlines separating pairs.

use crate::{
    client::{DadaClient, HeaderClient},
    errors::{PsrdadaError, PsrdadaResult},
};
use nom::{
    bytes::complete::{is_not, tag},
    character::complete::{line_ending, not_line_ending, space0, space1},
    combinator::opt,
    multi::{many0, many1, separated_list1},
    sequence::{preceded, separated_pair, terminated, tuple},
    IResult,
};
use psrdada_sys::ipcbuf_get_bufsz;
use std::{collections::HashMap, str};

type RawPair<'a> = (&'a [u8], &'a [u8]);

fn token(input: &[u8]) -> IResult<&[u8], &[u8]> {
    is_not(" \t\n\r#\0")(input)
}

fn pair(input: &[u8]) -> IResult<&[u8], RawPair> {
    terminated(
        separated_pair(token, space1, token),
        tuple((space0, opt(preceded(tag("#"), not_line_ending)))),
    )(input)
}

fn header(input: &[u8]) -> IResult<&[u8], Vec<RawPair>> {
    terminated(
        separated_list1(many1(line_ending), pair),
        tuple((many0(line_ending), opt(tag("\0")))),
    )(input)
}

/// Convert a `HashMap<String,String>` into a psrdada-compatible vector of bytes
///
/// Safety: there are limitations on what can be a key and a value. For example, neither
/// can contain spaces, tabs, newlines, #, or \0. We are not validating that here so you could
/// end up with bad bytes in the end.
pub unsafe fn header_to_bytes(header: &HashMap<String, String>) -> Vec<u8> {
    let mut bytes = vec![];
    for (k, v) in header {
        bytes.extend(k.as_bytes());
        bytes.extend(b" ");
        bytes.extend(v.as_bytes());
        bytes.extend(b"\n");
    }
    bytes
}

pub fn bytes_to_header(bytes: &[u8]) -> PsrdadaResult<HashMap<String, String>> {
    let (_, pairs) = header(bytes).map_err(|_| PsrdadaError::HeaderParseError)?;
    Ok(pairs
        .iter()
        .map(|(k, v)| {
            (
                str::from_utf8(*k)
                    .expect("We would've failed parsing earlier")
                    .to_owned(),
                str::from_utf8(*v)
                    .expect("We would've failed parsing earlier")
                    .to_owned(),
            )
        })
        .collect())
}

impl HeaderClient<'_> {
    pub unsafe fn push_header(&mut self, header: &HashMap<String, String>) -> PsrdadaResult<usize> {
        let bytes = header_to_bytes(header);
        let bufsz = ipcbuf_get_bufsz(*self.buf);
        let mut writer = self.writer();
        // Create a buffer of zeros, then copy over our header
        let mut whole_buffer = vec![0u8; bufsz as usize];
        (whole_buffer[0..bytes.len()]).copy_from_slice(&bytes);
        writer.push(&whole_buffer)
    }

    pub fn pop_header(&mut self) -> PsrdadaResult<HashMap<String, String>> {
        let mut reader = self.reader();
        let bytes = match reader.pop() {
            Some(b) => b,
            None => return Err(PsrdadaError::HeaderEodError),
        };
        bytes_to_header(&bytes)
    }
}

impl DadaClient {
    pub unsafe fn push_header(&mut self, header: &HashMap<String, String>) -> PsrdadaResult<usize> {
        let (mut hc, _) = self.split();
        hc.push_header(header)
    }

    pub fn pop_header(&mut self) -> PsrdadaResult<HashMap<String, String>> {
        let (mut hc, _) = self.split();
        hc.pop_header()
    }
}

#[cfg(test)]
mod tests {
    use crate::{builder::DadaClientBuilder, tests::next_key};

    use super::*;

    #[test]
    fn test_to_and_from_header() {
        let mut header = HashMap::new();
        header.insert("KEY".to_string(), "VALUE".to_string());
        header.insert("KEY2".to_string(), "VALUE2".to_string());
        let bytes = unsafe { header_to_bytes(&header) };
        let new_header = bytes_to_header(&bytes).unwrap();
        assert_eq!(header, new_header)
    }

    #[test]
    fn test_token_parser() {
        let (_, token) = token(b"A_TOKEN").unwrap();
        assert_eq!(b"A_TOKEN", token);
    }

    #[test]
    fn test_pair_parser() {
        let (_, pair) = pair(b"FOO_BAR 123.456").unwrap();
        assert_eq!(b"FOO_BAR", pair.0);
        assert_eq!(b"123.456", pair.1);
    }

    #[test]
    fn test_commented_pair_parser() {
        let (_, pair) = pair(b"FOO_BAR 123.456    # random nonsense foo barbaz").unwrap();
        assert_eq!(b"FOO_BAR", pair.0);
        assert_eq!(b"123.456", pair.1);
    }

    #[test]
    fn test_header_parser() {
        let hdr = b"FOO\tBAR # A comment\nBAZ   \tquuz123#morecomment__\n\nbEanS __RICE__";
        let (remaining, pairs) = header(hdr).unwrap();

        let p1 = pairs.get(0).unwrap();
        assert_eq!(b"FOO", p1.0);
        assert_eq!(b"BAR", p1.1);

        let p2 = pairs.get(1).unwrap();
        assert_eq!(b"BAZ", p2.0);
        assert_eq!(b"quuz123", p2.1);

        let p3 = pairs.get(2).unwrap();
        assert_eq!(b"bEanS", p3.0);
        assert_eq!(b"__RICE__", p3.1);

        assert_eq!(b"", remaining);
    }

    #[test]
    fn test_from_c_string() {
        let hdr = b"foo bar\nbaz buzz#foob\n\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
        let (_, pairs) = header(hdr).unwrap();

        let p1 = pairs.get(0).unwrap();
        assert_eq!(b"foo", p1.0);
        assert_eq!(b"bar", p1.1);

        let p2 = pairs.get(1).unwrap();
        assert_eq!(b"baz", p2.0);
        assert_eq!(b"buzz", p2.1);
    }

    #[test]
    fn test_bytes_to_header() {
        let hdr = b"foo bar\nbaz buzz";
        let hdr_parsed = bytes_to_header(hdr).unwrap();
        assert_eq!(
            HashMap::from([
                ("foo".to_owned(), "bar".to_owned()),
                ("baz".to_owned(), "buzz".to_owned()),
            ]),
            hdr_parsed
        )
    }

    #[test]
    fn test_roundtrip_header() {
        let key = next_key();
        let mut client = DadaClientBuilder::new(key).build().unwrap();

        let header = HashMap::from([
            ("foo".to_owned(), "bar".to_owned()),
            ("baz".to_owned(), "buzz".to_owned()),
        ]);

        // Push
        unsafe {
            client.push_header(&header).unwrap();
        }

        // Pop
        assert_eq!(header, client.pop_header().unwrap());
    }
}
