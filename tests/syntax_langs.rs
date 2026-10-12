//! Per-language tokenizer tests: keywords, strings, comments, numbers, operators and bracket depth for every
//! language mode added by `:setlang` (task Features 2 and 4). The older languages are tested in `src/syntax.rs`.

use kuro::syntax::{self, Kind, LangSpec};
use std::path::Path;

fn lines(src: &str) -> Vec<String> {
    src.split('\n').map(str::to_string).collect()
}

/// The kind of the first character of the first occurrence of `needle` on line `l`.
fn kind(spec: &LangSpec, src: &str, l: usize, needle: &str) -> Kind {
    let ks = syntax::highlight(spec, &lines(src));
    let line = &lines(src)[l];
    let at = line.find(needle).unwrap_or_else(|| panic!("{needle:?} is not on line {l}: {line:?}"));
    ks[l][line[..at].chars().count()]
}

/// Every character of `needle` has kind `want`.
fn all_kind(spec: &LangSpec, src: &str, l: usize, needle: &str, want: Kind) {
    let ks = syntax::highlight(spec, &lines(src));
    let line = &lines(src)[l];
    let at = line.find(needle).unwrap_or_else(|| panic!("{needle:?} is not on line {l}: {line:?}"));
    let from = line[..at].chars().count();
    for (i, c) in needle.chars().enumerate() {
        assert_eq!(ks[l][from + i], want, "char {c:?} (#{i}) of {needle:?} in {line:?}");
    }
}

fn check(spec: &LangSpec, src: &str, expect: &[(usize, &str, Kind)]) {
    for (l, needle, want) in expect {
        all_kind(spec, src, *l, needle, *want);
    }
}

fn by(name: &str) -> &'static LangSpec {
    syntax::by_name(name).unwrap_or_else(|| panic!("no mode {name}"))
}

#[test]
fn every_new_mode_is_found_by_name_alias_and_extension() {
    let cases = [
        ("rust", "rs", "main.rs", "Rust"),
        ("c", "c", "x.h", "C"),
        ("c++", "cpp", "x.cc", "C++"),
        ("java", "java", "A.java", "Java"),
        ("javascript", "js", "a.mjs", "JavaScript"),
        ("typescript", "ts", "a.ts", "TypeScript"),
        ("html", "htm", "index.htm", "HTML"),
        ("css", "css", "s.css", "CSS"),
        ("php", "phtml", "a.php", "PHP"),
        ("ruby", "rb", "a.rb", "Ruby"),
        ("yaml", "yml", "c.yaml", "YAML"),
        ("toml", "toml", "Cargo.toml", "TOML"),
        ("xml", "svg", "a.xml", "XML"),
    ];
    for (name, alias, file, want) in cases {
        assert_eq!(by(name).name, want);
        assert_eq!(by(alias).name, want, "alias {alias}");
        assert_eq!(by(&name.to_uppercase()).name, want, "case-insensitive");
        assert_eq!(syntax::detect(Some(Path::new(file)), "").name, want, "extension of {file}");
    }
    assert_eq!(syntax::detect(Some(Path::new("Rakefile")), "").name, "Ruby");
    assert_eq!(syntax::detect(Some(Path::new("a.mjs")), "").name, "JavaScript");
    assert_eq!(syntax::detect(None, "#!/usr/bin/env node").name, "JavaScript");
    assert_eq!(syntax::detect(None, "#!/usr/bin/ruby").name, "Ruby");
    assert_eq!(syntax::detect(None, "#!/usr/bin/env php").name, "PHP");
}

#[test]
fn every_mode_colours_bracket_depth_and_flags_mismatches_where_it_has_brackets() {
    for spec in syntax::LANGS {
        if matches!(spec.name, "HTML" | "XML") {
            continue; // no brackets in the markup itself
        }
        let src = match spec.name {
            "PHP" => "<?php f(g([1])); ?>",
            "CSS" => "a { b: c(d[1]); }",
            _ => "f(g([1]))",
        };
        let ks = &syntax::highlight(spec, &lines(src))[0];
        let depths: Vec<u8> = ks.iter().filter_map(|k| if let Kind::Bracket(d) = k { Some(*d) } else { None }).collect();
        assert!(
            depths.len() >= 4 && depths.windows(2).any(|w| w[0] != w[1]),
            "{}: nested brackets get different depths: {depths:?}",
            spec.name
        );
    }
    for (name, src) in [("rust", "f(]"), ("java", "x[1)"), ("css", "a { b: (c }")] {
        let ks = &syntax::highlight(by(name), &lines(src))[0];
        assert!(ks.contains(&Kind::BracketError), "{name}: {src}");
    }
}

// ------------------------------------------------------------------------------------------------ Rust

#[test]
fn rust() {
    let src = "fn main<'a>(x: &'a str) -> u8 { // note\n    let c = 'x'; let n = 0xFF_u8; let s = \"hi\"; /* b */ let r = 1..=2;\n}";
    let r = by("rust");
    check(
        r,
        src,
        &[
            (0, "fn", Kind::Keyword),
            (0, "u8", Kind::Builtin),
            (0, "'a", Kind::Builtin),
            (0, "->", Kind::Operator),
            (0, "// note", Kind::Comment),
            (1, "let", Kind::Keyword),
            (1, "'x'", Kind::String),
            (1, "0xFF_u8", Kind::Number),
            (1, "\"hi\"", Kind::String),
            (1, "/* b */", Kind::Comment),
            (1, "..=", Kind::Operator),
        ],
    );
    assert_eq!(kind(r, src, 1, "1..="), Kind::Number, "the range dots are not part of the number");
    assert_eq!(kind(r, "let e = '\\n';", 0, "'\\n'"), Kind::String, "escaped character literal");
    assert_eq!(kind(r, "impl<'static> X {}", 0, "'static"), Kind::Builtin);
}

// --------------------------------------------------------------------------------------------- C / C++

#[test]
fn c_and_cpp() {
    let src = "#include <stdio.h>\nint main(void) { // c\n  char *s = \"a\\\"b\"; long n = 0x1F + 2.5e3; return 0; /* x */ }";
    let c = by("c");
    check(
        c,
        src,
        &[
            (0, "#include", Kind::Keyword),
            (1, "int", Kind::Builtin),
            (1, "// c", Kind::Comment),
            (2, "char", Kind::Builtin),
            (2, "\"a\\\"b\"", Kind::String),
            (2, "0x1F", Kind::Number),
            (2, "2.5e3", Kind::Number),
            (2, "return", Kind::Keyword),
            (2, "/* x */", Kind::Comment),
        ],
    );
    assert_eq!(kind(c, "x = 'a';", 0, "'a'"), Kind::String);
    assert_eq!(kind(c, "  # define X 1", 0, "# define"), Kind::Keyword, "directive after indentation");
    assert_eq!(kind(c, "int a = b # c;", 0, "# c"), Kind::Plain, "a # elsewhere is not a directive");
    let p = by("c++");
    let cpp = "template <typename T> class A : public B { virtual void f() override; std::string s = nullptr; };";
    check(
        p,
        cpp,
        &[
            (0, "template", Kind::Keyword),
            (0, "class", Kind::Keyword),
            (0, "public", Kind::Keyword),
            (0, "override", Kind::Keyword),
            (0, "nullptr", Kind::Literal),
            (0, "::", Kind::Operator),
        ],
    );
}

// ------------------------------------------------------------------------------------------------ Java

#[test]
fn java() {
    let src = "@Override\npublic static void main(String[] a) { int n = 42; String s = \"x\"; // c\n  if (n >= 1 && s != null) {} }";
    let j = by("java");
    check(
        j,
        src,
        &[
            (0, "@Override", Kind::Builtin),
            (1, "public", Kind::Keyword),
            (1, "static", Kind::Keyword),
            (1, "void", Kind::Builtin),
            (1, "42", Kind::Number),
            (1, "\"x\"", Kind::String),
            (1, "// c", Kind::Comment),
            (2, "if", Kind::Keyword),
            (2, ">=", Kind::Operator),
            (2, "&&", Kind::Operator),
            (2, "null", Kind::Literal),
        ],
    );
}

// ------------------------------------------------------------------------------------------ JavaScript

#[test]
fn javascript() {
    let src = "const re = /ab+c/gi.test(`t ${x}`); // c\nlet n = 0b101 + 1_000 + 10n; const q = a / b / 2;\nreturn /x\\/y[/]/.exec(s);";
    let js = by("js");
    check(
        js,
        src,
        &[
            (0, "const", Kind::Keyword),
            (0, "/ab+c/gi", Kind::String),
            (0, "`t ${x}`", Kind::String),
            (0, "// c", Kind::Comment),
            (1, "let", Kind::Keyword),
            (1, "0b101", Kind::Number),
            (1, "1_000", Kind::Number),
            (1, "10n", Kind::Number),
            (2, "return", Kind::Keyword),
            (2, "/x\\/y[/]/", Kind::String),
        ],
    );
    // division is not a regex
    assert_eq!(kind(js, src, 1, "/ b"), Kind::Operator);
    assert_eq!(kind(js, src, 1, "/ 2"), Kind::Operator);
    assert_eq!(kind(js, "x = a / b / c;", 0, "/ c"), Kind::Operator);
    assert_eq!(kind(js, "x = a / b / c;", 0, "b"), Kind::Plain, "so what is between the slashes is not a string");
    assert_eq!(kind(js, "if (a) /re/.test(b)", 0, "/re/"), Kind::Operator, "after `)` a slash divides (a known limit)");
    assert_eq!(kind(js, "x = f(/re/)", 0, "/re/"), Kind::String);
    assert_eq!(kind(js, "x = [/a/, /b/]", 0, "/b/"), Kind::String);
}

#[test]
fn template_strings_span_lines() {
    let src = "const t = `line one\nline two ${x}\nend`; let y = 1;";
    let js = by("javascript");
    all_kind(js, src, 1, "line two ${x}", Kind::String);
    all_kind(js, src, 2, "end`", Kind::String);
    assert_eq!(kind(js, src, 2, "let"), Kind::Keyword, "code resumes after the closing backtick");
}

#[test]
fn typescript() {
    let src = "interface A { x: number; readonly y?: string }\nexport type T = keyof A; const v = <any>1 as unknown; // c";
    let ts = by("ts");
    check(
        ts,
        src,
        &[
            (0, "interface", Kind::Keyword),
            (0, "number", Kind::Builtin),
            (0, "readonly", Kind::Keyword),
            (1, "export", Kind::Keyword),
            (1, "type", Kind::Keyword),
            (1, "keyof", Kind::Keyword),
            (1, "unknown", Kind::Builtin),
            (1, "// c", Kind::Comment),
        ],
    );
}

// ------------------------------------------------------------------------------------------------ Ruby

#[test]
fn ruby() {
    let src = "class Foo < Bar # c\n  def go(x = nil) = puts \"hi #{x}\"\n  @a = 3.14; $g = :sym; 'q'\nend";
    let rb = by("ruby");
    check(
        rb,
        src,
        &[
            (0, "class", Kind::Keyword),
            (0, "# c", Kind::Comment),
            (1, "def", Kind::Keyword),
            (1, "nil", Kind::Literal),
            (1, "puts", Kind::Builtin),
            (1, "\"hi #{x}\"", Kind::String),
            (2, "@a", Kind::Builtin),
            (2, "3.14", Kind::Number),
            (2, "$g", Kind::Builtin),
            (2, "'q'", Kind::String),
            (3, "end", Kind::Keyword),
        ],
    );
}

// --------------------------------------------------------------------------------------- YAML / TOML

#[test]
fn yaml() {
    let src = "# top\nname: kuro\nitems:\n  - key: 'v'  # c\n  - 42\nflag: true\nurl: http://x:80/y\n\"quoted\": [1, 2]";
    let y = by("yml");
    check(
        y,
        src,
        &[
            (0, "# top", Kind::Comment),
            (1, "name", Kind::Key),
            (2, "items", Kind::Key),
            (3, "key", Kind::Key),
            (3, "'v'", Kind::String),
            (3, "# c", Kind::Comment),
            (4, "42", Kind::Number),
            (5, "flag", Kind::Key),
            (5, "true", Kind::Literal),
            (6, "url", Kind::Key),
        ],
    );
    assert_eq!(kind(y, src, 1, "kuro"), Kind::Plain, "plain scalars stay plain");
    assert_eq!(kind(y, src, 6, "http"), Kind::Plain, "a colon not followed by a space is not a key separator");
    let ks = syntax::highlight(y, &lines(src));
    assert!(ks[7].iter().any(|k| matches!(k, Kind::Bracket(0))), "flow sequences get bracket colours");
}

#[test]
fn toml() {
    let src = "# c\n[package.meta]\nname = \"kuro\"  # trailing\nversion = '0.1'\nn = 1_000\nok = true\nlist = [1, 2.5, \"x\"]\ndoc = \"\"\"multi\nline\"\"\"\ndotted.key = 1";
    let t = by("toml");
    check(
        t,
        src,
        &[
            (0, "# c", Kind::Comment),
            (1, "[package.meta]", Kind::Keyword),
            (2, "name", Kind::Key),
            (2, "\"kuro\"", Kind::String),
            (2, "# trailing", Kind::Comment),
            (3, "'0.1'", Kind::String),
            (4, "1_000", Kind::Number),
            (5, "true", Kind::Literal),
            (6, "2.5", Kind::Number),
            (8, "line\"\"\"", Kind::String),
            (9, "dotted.key", Kind::Key),
        ],
    );
}

// --------------------------------------------------------------------------------------------- HTML

const PAGE: &str = "<!DOCTYPE html>\n<!-- hi -->\n<html lang=\"en\">\n<a href='x' disabled class=btn>t &amp; &#169; u</a>\n<br/>\n<style>\n/* c */ p.x:hover { color: #fff; margin: 0 -1.5em; }\n</style>\n<script>\nconst a = /re/; // js\nlet s = \"q\";\n</script>\n</html>";

#[test]
fn html_tags_attributes_comments_entities_and_doctype() {
    let h = by("html");
    check(
        h,
        PAGE,
        &[
            (0, "<!DOCTYPE html>", Kind::Keyword),
            (1, "<!-- hi -->", Kind::Comment),
            (2, "<", Kind::Operator),
            (2, "html", Kind::Keyword),
            (2, "lang", Kind::Key),
            (2, "=", Kind::Operator),
            (2, "\"en\"", Kind::String),
            (2, ">", Kind::Operator),
            (3, "a", Kind::Keyword),
            (3, "href", Kind::Key),
            (3, "'x'", Kind::String),
            (3, "disabled", Kind::Key),
            (3, "class", Kind::Key),
            (3, "btn", Kind::String),
            (3, "&amp;", Kind::Literal),
            (3, "&#169;", Kind::Literal),
            (3, "</", Kind::Operator),
            (4, "br", Kind::Keyword),
            (4, "/>", Kind::Operator),
        ],
    );
    assert_eq!(kind(h, PAGE, 3, "t &amp;"), Kind::Plain, "text between tags is plain");
    assert_eq!(kind(h, "a & b &x", 0, "& b"), Kind::Plain, "a bare ampersand is not an entity");
}

#[test]
fn style_and_script_regions_are_coloured_as_css_and_javascript() {
    let h = by("html");
    check(
        h,
        PAGE,
        &[
            (5, "style", Kind::Keyword),
            (6, "/* c */", Kind::Comment),
            (6, "p", Kind::Keyword),
            (6, ".x", Kind::Literal),
            (6, ":hover", Kind::Builtin),
            (6, "color", Kind::Key),
            (6, "#fff", Kind::Number),
            (6, "-1.5em", Kind::Number),
            (9, "const", Kind::Keyword),
            (9, "/re/", Kind::String),
            (9, "// js", Kind::Comment),
            (10, "\"q\"", Kind::String),
            (11, "script", Kind::Keyword),
        ],
    );
    // a comment inside <script> does not leak past </script>
    let src = "<script>/* open\n</script>\n<p>after</p>";
    assert_eq!(kind(h, src, 2, "p"), Kind::Keyword);
    // a script that is never closed runs to the end
    assert_eq!(kind(h, "<script>\nvar x = 1;", 1, "var"), Kind::Keyword);
    // JSON and unknown script types
    let json = "<script type=\"application/json\">{\"a\": 1}</script>";
    assert_eq!(kind(h, json, 0, "\"a\""), Kind::Key);
    let tpl = "<script type=\"text/template\">var x = 1</script>";
    assert_eq!(kind(h, tpl, 0, "var"), Kind::Plain, "unknown script types are left alone");
}

#[test]
fn unfinished_markup_never_panics_and_stays_plain_where_unknown() {
    let h = by("html");
    for src in [
        "<",
        "<a",
        "<a href=\"",
        "<!--",
        "<!-- x",
        "<style",
        "<style>p {",
        "<script>/*",
        "</",
        "<<<>>>",
        "&",
        "&#",
        "a < b > c",
        "<a b=c d='e' f=\"g\"",
        "<?php",
        "<a\n\n\nhref=x",
    ] {
        let ks = syntax::highlight(h, &lines(src));
        assert_eq!(ks.len(), lines(src).len());
        for (k, l) in ks.iter().zip(lines(src)) {
            assert_eq!(k.len(), l.chars().count(), "{src:?}");
        }
    }
    assert_eq!(kind(h, "a < b > c", 0, "< b"), Kind::Plain);
}

// ----------------------------------------------------------------------------------------------- CSS

#[test]
fn css_selectors_properties_values_and_at_rules() {
    let css = "@media (max-width: 600px) {\n  .nav > li:first-child, #id[data-x=\"y\"] {\n    background: url(\"a.png\") no-repeat;\n    margin: 0 auto !important; width: calc(100% - 2rem);\n    --main-color: #FF00aa; color: rgba(0, 0, 0, .5);\n  }\n}\n/* done */\nbody { font-family: \"Helvetica Neue\", sans-serif }";
    let c = by("css");
    check(
        c,
        css,
        &[
            (0, "@media", Kind::Keyword),
            (0, "max-width", Kind::Key),
            (0, "600px", Kind::Number),
            (1, ".nav", Kind::Literal),
            (1, ">", Kind::Operator),
            (1, "li", Kind::Keyword),
            (1, ":first-child", Kind::Builtin),
            (1, "#id", Kind::Literal),
            (1, "\"y\"", Kind::String),
            (2, "background", Kind::Key),
            (2, "url", Kind::Builtin),
            (2, "\"a.png\"", Kind::String),
            (3, "margin", Kind::Key),
            (3, "0", Kind::Number),
            (3, "!important", Kind::Keyword),
            (3, "width", Kind::Key),
            (3, "calc", Kind::Builtin),
            (3, "100%", Kind::Number),
            (3, "2rem", Kind::Number),
            (4, "--main-color", Kind::Key),
            (4, "#FF00aa", Kind::Number),
            (4, "rgba", Kind::Builtin),
            (4, ".5", Kind::Number),
            (7, "/* done */", Kind::Comment),
            (8, "body", Kind::Keyword),
            (8, "font-family", Kind::Key),
            (8, "\"Helvetica Neue\"", Kind::String),
        ],
    );
    assert_eq!(kind(c, css, 2, "no-repeat"), Kind::Plain, "value keywords stay plain");
    assert_eq!(kind(c, css, 8, "sans-serif"), Kind::Plain);
}

#[test]
fn css_comments_span_lines_and_a_pseudo_class_is_not_a_property() {
    let c = by("css");
    let src = "/* a\nb */ a:hover { color: red }\ndiv:not(.x) span::before { content: \"x\" }";
    all_kind(c, src, 0, "/* a", Kind::Comment);
    all_kind(c, src, 1, "b */", Kind::Comment);
    check(
        c,
        src,
        &[
            (1, "a", Kind::Keyword),
            (1, ":hover", Kind::Builtin),
            (1, "color", Kind::Key),
            (2, "div", Kind::Keyword),
            (2, ":not", Kind::Builtin),
            (2, ".x", Kind::Literal),
            (2, "span", Kind::Keyword),
            (2, "::before", Kind::Builtin),
            (2, "content", Kind::Key),
        ],
    );
}

// ------------------------------------------------------------------------------------------------ PHP

const PHP_PAGE: &str = "<html>\n<?php\n// c\n$name = \"kuro\"; echo $name . 42;\nif ($a >= 1) { return null; } # t\n/* b */ ?>\n<p><?= $x ?></p>\n<?php foreach ($xs as $x): ?>\n<b>x</b>\n<?php endforeach; ?>";

#[test]
fn php_variables_keywords_strings_comments_and_tag_boundaries() {
    let p = by("php");
    check(
        p,
        PHP_PAGE,
        &[
            (0, "html", Kind::Keyword),
            (1, "<?php", Kind::Keyword),
            (2, "// c", Kind::Comment),
            (3, "$name", Kind::Builtin),
            (3, "\"kuro\"", Kind::String),
            (3, "echo", Kind::Keyword),
            (3, ".", Kind::Operator),
            (3, "42", Kind::Number),
            (4, "if", Kind::Keyword),
            (4, "$a", Kind::Builtin),
            (4, ">=", Kind::Operator),
            (4, "null", Kind::Literal),
            (4, "# t", Kind::Comment),
            (5, "/* b */", Kind::Comment),
            (5, "?>", Kind::Keyword),
            (6, "<?=", Kind::Keyword),
            (6, "$x", Kind::Builtin),
            (6, "?>", Kind::Keyword),
            (6, "p", Kind::Keyword),
            (7, "foreach", Kind::Keyword),
            (8, "b", Kind::Keyword),
            (9, "endforeach", Kind::Keyword),
        ],
    );
    assert_eq!(kind(p, PHP_PAGE, 8, "x</b>"), Kind::Plain, "HTML text between PHP regions is plain");
}

#[test]
fn php_inside_attributes_and_a_php_file_that_never_closes() {
    let p = by("php");
    let src = "<a href=\"<?php echo $u; ?>\" class=x>";
    assert_eq!(kind(p, src, 0, "class"), Kind::Key);
    // inside a double-quoted attribute value the PHP region is not special (it is part of the string)
    let open = "<?php\n$a = 1;\nfunction f() { return $a; }";
    check(p, open, &[(1, "$a", Kind::Builtin), (2, "function", Kind::Keyword), (2, "return", Kind::Keyword)]);
    // a block comment does not leak out of the PHP region
    let leak = "<?php /* open ?>\n<p>x</p>";
    assert_eq!(kind(p, leak, 1, "p"), Kind::Keyword);
}

// ------------------------------------------------------------------------------------------------ XML

#[test]
fn xml_declarations_tags_attributes_cdata_and_comments() {
    let src = "<?xml version=\"1.0\"?>\n<!-- c -->\n<root xmlns:a=\"u\">\n  <a:item id='1'/><![CDATA[ <raw> ]]>\n  text &lt; more\n</root>";
    let x = by("xml");
    check(
        x,
        src,
        &[
            (0, "<?xml version=\"1.0\"?>", Kind::Builtin),
            (1, "<!-- c -->", Kind::Comment),
            (2, "root", Kind::Keyword),
            (2, "xmlns:a", Kind::Key),
            (2, "\"u\"", Kind::String),
            (3, "a:item", Kind::Keyword),
            (3, "id", Kind::Key),
            (3, "'1'", Kind::String),
            (3, "/>", Kind::Operator),
            (3, "<![CDATA[ <raw> ]]>", Kind::String),
            (4, "&lt;", Kind::Literal),
            (5, "root", Kind::Keyword),
        ],
    );
    // XML has no embedded languages: <style> content is just text
    assert_eq!(kind(x, "<style>p { color: red }</style>", 0, "color"), Kind::Plain);
}

// ------------------------------------------------------------------------------------ cross-language

#[test]
fn every_language_survives_hostile_input_and_keeps_line_lengths() {
    let nasty = [
        "",
        "\u{0}\u{1}\t\t  ",
        "\"unterminated",
        "'x",
        "`never closed",
        "/* open",
        "<<<<<>>>>>",
        "((((((((((((((((((((",
        "}}}}]]]]))))",
        "日本語 \"日本語\" // 日本語",
        "a\nb\n\n\n",
        &"x".repeat(5000),
        "<?php /* <style> <script> `",
        "#[derive(Debug)] 'a 'b' '\\",
        "/re/ /[/ /\\/",
    ];
    for spec in syntax::LANGS {
        for src in nasty {
            let ls = lines(src);
            let ks = syntax::highlight(spec, &ls);
            assert_eq!(ks.len(), ls.len(), "{}: line count for {src:?}", spec.name);
            for (k, l) in ks.iter().zip(&ls) {
                assert_eq!(k.len(), l.chars().count(), "{}: line length for {src:?}", spec.name);
            }
        }
    }
}

#[test]
fn no_language_uses_a_colour_role_that_does_not_exist() {
    // all roles map to palette slots in `[colors]`; this guards against someone adding a Kind without a colour
    let roles =
        [Kind::Plain, Kind::Keyword, Kind::Builtin, Kind::Literal, Kind::String, Kind::Number, Kind::Comment, Kind::Key, Kind::Operator];
    let sample = "<a href=\"x\">&amp;</a> fn f() { 1 + 'a' } // c";
    for spec in syntax::LANGS {
        for line in syntax::highlight(spec, &lines(sample)) {
            for k in line {
                assert!(roles.contains(&k) || matches!(k, Kind::Bracket(_) | Kind::BracketError));
            }
        }
    }
}
