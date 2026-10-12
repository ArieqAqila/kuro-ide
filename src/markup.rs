//! Tokenizers for markup languages: HTML, XML, PHP (HTML with `<?php ?>` regions) and CSS.
//!
//! These cannot be described by the table-driven tokenizer (context decides what a word is), so each is a small
//! state machine over the whole text. They do **not** parse: they colour what is recognisable and leave the rest
//! plain, so a half-written file is never a problem.
//!
//! Colour roles (all mapped to terminal palette slots by `[colors]`):
//!
//! | Where | Role |
//! |---|---|
//! | tag names, `<!DOCTYPE>`, PHP `<?php` / `?>`, CSS element selectors and `@rules` | keyword |
//! | attribute names, CSS properties | key |
//! | attribute values, CDATA | string |
//! | `<`, `>`, `/>`, `=`, CSS combinators | operator |
//! | HTML/XML comments, CSS comments | comment |
//! | `&amp;` entities, CSS `.class` and `#id` selectors | literal |
//! | CSS numbers with units and `#rrggbb` colours | number |
//! | CSS pseudo-classes and function names, XML processing instructions | builtin |
//!
//! Embedded regions are handed to the language they contain: `<style>` to CSS, `<script>` to JavaScript (or JSON when
//! `type` says so), `<?php ... ?>` to PHP. Each region is tokenized on its own, so a comment or a bracket never leaks
//! out of its region; a region that is never closed runs to the end of the file.

use crate::syntax::{self, Kind, LangSpec, Markup};

pub fn highlight(spec: &LangSpec, m: Markup, lines: &[String]) -> Vec<Vec<Kind>> {
    let mut text: Vec<char> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.extend(l.chars());
    }
    let mut kinds = vec![Kind::Plain; text.len()];
    match m {
        Markup::Css => css(&text, 0, text.len(), &mut kinds),
        _ => markup(spec, m, &text, 0, text.len(), &mut kinds),
    }
    // split back into lines
    let mut out = Vec::with_capacity(lines.len());
    let mut at = 0;
    for l in lines {
        let n = l.chars().count();
        out.push(kinds[at..at + n].to_vec());
        at += n + 1;
    }
    out
}

fn starts(t: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| t.get(i + k) == Some(&p))
}

fn starts_ci(t: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| t.get(i + k).is_some_and(|c| c.eq_ignore_ascii_case(&p)))
}

fn find(t: &[char], from: usize, end: usize, pat: &str, ci: bool) -> Option<usize> {
    (from..end).find(|&i| if ci { starts_ci(t, i, pat) } else { starts(t, i, pat) })
}

fn fill(k: &mut [Kind], a: usize, b: usize, kind: Kind) {
    let b = b.min(k.len());
    for x in k.iter_mut().take(b).skip(a) {
        *x = kind;
    }
}

/// Tokenize `t[a..b]` as `lang` and write the kinds back (newlines keep `Plain`).
fn embed(lang: &LangSpec, t: &[char], a: usize, b: usize, k: &mut [Kind]) {
    if a >= b {
        return;
    }
    let region: String = t[a..b].iter().collect();
    let lines: Vec<String> = region.split('\n').map(str::to_string).collect();
    let kinds = syntax::highlight(lang, &lines);
    let mut at = a;
    for line in kinds {
        for kind in &line {
            k[at] = *kind;
            at += 1;
        }
        at += 1; // the newline
    }
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.')
}

/// A `<?php ... ?>` (or `<?= ... ?>`) region starting at `i`; returns where to continue.
fn php_region(t: &[char], i: usize, end: usize, k: &mut [Kind]) -> usize {
    let open = if starts_ci(t, i, "<?php") {
        5
    } else if starts(t, i, "<?=") {
        3
    } else {
        2
    };
    fill(k, i, i + open, Kind::Keyword);
    let code = i + open;
    match find(t, code, end, "?>", false) {
        Some(c) => {
            embed(&syntax::PHP_CODE, t, code, c, k);
            fill(k, c, c + 2, Kind::Keyword);
            c + 2
        }
        None => {
            embed(&syntax::PHP_CODE, t, code, end, k);
            end
        }
    }
}

struct Tag {
    next: usize,
    name: String,
    closing: bool,
    self_closing: bool,
    type_attr: Option<String>,
}

fn tag(m: Markup, t: &[char], start: usize, end: usize, k: &mut [Kind]) -> Tag {
    let mut i = start + 1;
    let closing = t.get(i) == Some(&'/');
    if closing {
        i += 1;
    }
    fill(k, start, i, Kind::Operator);
    let name_start = i;
    while i < end && is_name(t[i]) {
        i += 1;
    }
    fill(k, name_start, i, Kind::Keyword);
    let name: String = t[name_start..i].iter().collect::<String>().to_ascii_lowercase();
    let mut type_attr = None;
    let mut self_closing = false;
    let mut after_equals = false; // the next word is an unquoted attribute value
    while i < end {
        let c = t[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '>' {
            fill(k, i, i + 1, Kind::Operator);
            i += 1;
            break;
        } else if starts(t, i, "/>") {
            fill(k, i, i + 2, Kind::Operator);
            self_closing = true;
            i += 2;
            break;
        } else if m == Markup::Php && starts(t, i, "<?") {
            i = php_region(t, i, end, k);
        } else if c == '<' {
            break; // a tag that was never closed: the next tag starts here
        } else if c == '=' {
            fill(k, i, i + 1, Kind::Operator);
            after_equals = true;
            i += 1;
        } else if c == '"' || c == '\'' {
            after_equals = false;
            let close = (i + 1..end).find(|&j| t[j] == c).map(|j| j + 1).unwrap_or(end);
            fill(k, i, close, Kind::String);
            i = close;
        } else {
            let a = i;
            while i < end && !t[i].is_whitespace() && !matches!(t[i], '=' | '>' | '<' | '"' | '\'') && !starts(t, i, "/>") {
                i += 1;
            }
            let word: String = t[a..i].iter().collect();
            if after_equals {
                after_equals = false;
                fill(k, a, i, Kind::String); // unquoted value
                continue;
            }
            // the value of `type=...` (quoted or not), remembered for <script>
            let is_type = word.eq_ignore_ascii_case("type");
            fill(k, a, i, Kind::Key);
            let mut j = i;
            while j < end && t[j] == ' ' {
                j += 1;
            }
            if is_type && t.get(j) == Some(&'=') {
                j += 1;
                while j < end && t[j] == ' ' {
                    j += 1;
                }
                let (v0, v1) = match t.get(j) {
                    Some(&q) if q == '"' || q == '\'' => {
                        let e = (j + 1..end).find(|&x| t[x] == q).unwrap_or(end);
                        (j + 1, e)
                    }
                    _ => {
                        let mut e = j;
                        while e < end && !t[e].is_whitespace() && t[e] != '>' {
                            e += 1;
                        }
                        (j, e)
                    }
                };
                type_attr = Some(t[v0..v1].iter().collect::<String>().to_ascii_lowercase());
            }
        }
    }
    Tag { next: i, name, closing, self_closing, type_attr }
}

fn markup(_spec: &LangSpec, m: Markup, t: &[char], mut i: usize, end: usize, k: &mut [Kind]) {
    while i < end {
        let c = t[i];
        if c == '<' {
            if starts(t, i, "<!--") {
                let e = find(t, i + 4, end, "-->", false).map(|x| x + 3).unwrap_or(end);
                fill(k, i, e, Kind::Comment);
                i = e;
            } else if m == Markup::Php && starts(t, i, "<?") {
                i = php_region(t, i, end, k);
            } else if m == Markup::Xml && starts(t, i, "<?") {
                let e = find(t, i + 2, end, "?>", false).map(|x| x + 2).unwrap_or(end);
                fill(k, i, e, Kind::Builtin);
                i = e;
            } else if starts(t, i, "<![CDATA[") {
                let e = find(t, i + 9, end, "]]>", false).map(|x| x + 3).unwrap_or(end);
                fill(k, i, e, Kind::String);
                i = e;
            } else if starts(t, i, "<!") {
                let e = (i..end).find(|&j| t[j] == '>').map(|j| j + 1).unwrap_or(end);
                fill(k, i, e, Kind::Keyword);
                i = e;
            } else if t.get(i + 1).is_some_and(|x| x.is_alphabetic())
                || (t.get(i + 1) == Some(&'/') && t.get(i + 2).is_some_and(|x| x.is_alphabetic()))
            {
                let tg = tag(m, t, i, end, k);
                i = tg.next.max(i + 1);
                if m != Markup::Xml && !tg.closing && !tg.self_closing && (tg.name == "style" || tg.name == "script") {
                    let close_pat = format!("</{}", tg.name);
                    let e = find(t, i, end, &close_pat, true).unwrap_or(end);
                    match tg.name.as_str() {
                        "style" => css(t, i, e, k),
                        _ => match tg.type_attr.as_deref() {
                            None | Some("") | Some("module") | Some("text/javascript") | Some("application/javascript") => {
                                embed(&syntax::JAVASCRIPT, t, i, e, k)
                            }
                            Some("application/json") | Some("application/ld+json") | Some("importmap") => embed(&syntax::JSON, t, i, e, k),
                            _ => {}
                        },
                    }
                    i = e;
                }
            } else {
                i += 1;
            }
        } else if c == '&' {
            // entity: &name; or &#123; or &#x1F;
            let mut j = i + 1;
            if t.get(j) == Some(&'#') {
                j += 1;
            }
            let body = j;
            while j < end && t[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j > body && t.get(j) == Some(&';') && j - i <= 10 {
                fill(k, i, j + 1, Kind::Literal);
                i = j + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

// ----------------------------------------------------------------------------------------------- CSS

fn css_ident(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_')
}

/// Is the word ending at `j` a property name (`name: value;`) rather than a selector (`a:hover { ... }`)?
fn is_property(t: &[char], mut j: usize, end: usize) -> bool {
    while j < end && (t[j] == ' ' || t[j] == '\t') {
        j += 1;
    }
    if t.get(j) != Some(&':') || j >= end {
        return false;
    }
    for &x in &t[j + 1..end] {
        match x {
            ';' | '}' => return true,
            '{' => return false,
            _ => {}
        }
    }
    true // the last declaration of an unfinished block
}

fn css(t: &[char], a: usize, end: usize, k: &mut [Kind]) {
    let mut i = a;
    // open brackets: (char, position, a media-feature parenthesis whose `name: value` ends at its `)`)
    let mut stack: Vec<(char, usize, bool)> = Vec::new();
    let mut block = 0usize; // `{` depth
    let mut in_value = false;
    while i < end {
        let c = t[i];
        if c.is_whitespace() {
            i += 1;
        } else if starts(t, i, "/*") {
            let e = find(t, i + 2, end, "*/", false).map(|x| x + 2).unwrap_or(end);
            fill(k, i, e, Kind::Comment);
            i = e;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < end && t[j] != c && t[j] != '\n' {
                if t[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let e = if t.get(j) == Some(&c) { j + 1 } else { j.min(end) };
            fill(k, i, e, Kind::String);
            i = e;
        } else if matches!(c, '{' | '(' | '[') {
            k[i] = Kind::Bracket(stack.len().min(255) as u8);
            stack.push((c, i, false));
            if c == '{' {
                block += 1;
                in_value = false;
            }
            i += 1;
        } else if matches!(c, '}' | ')' | ']') {
            let want = match c {
                '}' => '{',
                ')' => '(',
                _ => '[',
            };
            if stack.last().is_some_and(|(o, ..)| *o == want) {
                if stack.pop().is_some_and(|(_, _, feature)| feature) {
                    in_value = false;
                }
                k[i] = Kind::Bracket(stack.len().min(255) as u8);
            } else {
                k[i] = Kind::BracketError;
            }
            if c == '}' {
                block = block.saturating_sub(1);
                in_value = false;
            }
            i += 1;
        } else if c == ';' {
            in_value = false;
            i += 1;
        } else if c == '@' && t.get(i + 1).is_some_and(|x| x.is_alphabetic()) {
            let mut j = i + 1;
            while j < end && css_ident(t[j]) {
                j += 1;
            }
            fill(k, i, j, Kind::Keyword);
            i = j;
        } else if c == '!' && starts_ci(t, i, "!important") {
            fill(k, i, i + 10, Kind::Keyword);
            i += 10;
        } else if in_value {
            if c == '#' && t.get(i + 1).is_some_and(|x| x.is_ascii_hexdigit()) {
                let mut j = i + 1;
                while j < end && t[j].is_ascii_hexdigit() {
                    j += 1;
                }
                fill(k, i, j, Kind::Number);
                i = j;
            } else if c.is_ascii_digit()
                || (c == '.' && t.get(i + 1).is_some_and(|x| x.is_ascii_digit()))
                || (matches!(c, '-' | '+')
                    && t.get(i + 1).is_some_and(|x| x.is_ascii_digit() || *x == '.')
                    && !i.checked_sub(1).is_some_and(|p| css_ident(t[p])))
            {
                let mut j = i + 1;
                while j < end && (t[j].is_ascii_digit() || t[j] == '.') {
                    j += 1;
                }
                while j < end && (t[j].is_ascii_alphabetic() || t[j] == '%') {
                    j += 1; // the unit
                }
                fill(k, i, j, Kind::Number);
                i = j;
            } else if css_ident(c) {
                let mut j = i;
                while j < end && css_ident(t[j]) {
                    j += 1;
                }
                let word: String = t[i..j].iter().collect();
                if word.starts_with("--") {
                    fill(k, i, j, Kind::Key); // custom property used as var(--x)
                } else if t.get(j) == Some(&'(') {
                    fill(k, i, j, Kind::Builtin); // function: rgba(), url(), calc(), var()
                }
                i = j;
            } else {
                if matches!(c, ',' | '/' | '*' | '+' | ':') {
                    k[i] = Kind::Operator;
                }
                i += 1;
            }
        } else if css_ident(c) && !c.is_ascii_digit() || (c == '-' && t.get(i + 1).is_some_and(|x| css_ident(*x))) {
            let mut j = i;
            while j < end && css_ident(t[j]) {
                j += 1;
            }
            let feature = !in_value && stack.last().is_some_and(|(o, ..)| *o == '(') && {
                let mut x = j;
                while x < end && t[x] == ' ' {
                    x += 1;
                }
                t.get(x) == Some(&':')
            };
            if feature {
                // `@media (max-width: 600px)`: a feature name, then its value up to the closing parenthesis
                fill(k, i, j, Kind::Key);
                let mut x = j;
                while x < end && t[x] != ':' {
                    x += 1;
                }
                k[x.min(end - 1)] = Kind::Operator;
                if let Some(top) = stack.last_mut() {
                    top.2 = true;
                }
                in_value = true;
                i = (x + 1).min(end);
            } else if block > 0 && is_property(t, j, end) {
                fill(k, i, j, Kind::Key);
                // the colon
                let mut x = j;
                while x < end && t[x] != ':' {
                    x += 1;
                }
                if x < end {
                    k[x] = Kind::Operator;
                    i = x + 1;
                } else {
                    i = j;
                }
                in_value = true;
            } else {
                fill(k, i, j, Kind::Keyword); // element selector
                i = j;
            }
        } else if (c == '.' || c == '#') && t.get(i + 1).is_some_and(|x| css_ident(*x)) {
            let mut j = i + 1;
            while j < end && css_ident(t[j]) {
                j += 1;
            }
            fill(k, i, j, Kind::Literal);
            i = j;
        } else if c == ':' {
            let mut j = i + 1;
            if t.get(j) == Some(&':') {
                j += 1;
            }
            while j < end && css_ident(t[j]) {
                j += 1;
            }
            fill(k, i, j, Kind::Builtin); // :hover, ::before, :nth-child
            i = j.max(i + 1);
        } else if matches!(c, '>' | '+' | '~' | '*' | ',' | '=' | '&' | '|' | '^' | '$') {
            k[i] = Kind::Operator;
            i += 1;
        } else {
            i += 1;
        }
    }
    for (_, p, _) in stack {
        k[p] = Kind::BracketError;
    }
}
