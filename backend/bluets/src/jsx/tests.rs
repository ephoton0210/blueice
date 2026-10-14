use super::*;

/// A stand-in for the lexer's expression scanner: the matching `}` by counting
/// braces (enough for these inputs, which hold no strings with braces).
fn braces(source: &str) -> impl Fn(usize) -> Result<(usize, Vec<Token>), String> + '_ {
    move |from| {
        let mut depth = 0usize;
        for (offset, byte) in source.bytes().enumerate().skip(from) {
            match byte {
                b'{' => depth += 1,
                b'}' if depth == 0 => return Ok((offset, Vec::new())),
                b'}' => depth -= 1,
                _ => {}
            }
        }
        Err("unterminated expression".to_string())
    }
}

fn parse(source: &str) -> Result<JsxElement, JsxError> {
    parse_element(source, 0, &braces(source))
}

#[test]
fn scans_elements_attributes_children_and_fragments() {
    let source = "<div id=\"a\" data-x={1 + 2} {...rest} hidden>hi {name}<b/></div>";
    let element = parse(source).unwrap();
    assert_eq!(element.name.as_ref().unwrap().text, "div");
    assert_eq!(element.end, source.len());
    assert_eq!(element.attributes.len(), 4);
    assert!(
        matches!(&element.attributes[0], JsxAttribute::Named { name, value: Some(JsxValue::String { .. }), .. } if name.text == "id")
    );
    assert!(
        matches!(&element.attributes[1], JsxAttribute::Named { name, value: Some(JsxValue::Expression { .. }), .. } if name.text == "data-x")
    );
    assert!(matches!(
        &element.attributes[2],
        JsxAttribute::Spread { .. }
    ));
    assert!(
        matches!(&element.attributes[3], JsxAttribute::Named { name, value: None, .. } if name.text == "hidden")
    );
    assert_eq!(element.children.len(), 3);
    assert!(matches!(&element.children[0], JsxChild::Text { .. }));
    assert!(matches!(
        &element.children[1],
        JsxChild::Expression { spread: false, .. }
    ));
    assert!(matches!(&element.children[2], JsxChild::Element(child) if child.self_closing));

    let fragment = parse("<><a/>text</>").unwrap();
    assert!(fragment.is_fragment());
    assert_eq!(fragment.children.len(), 2);
}

#[test]
fn names_may_be_member_namespaced_or_hyphenated() {
    for name in ["a.b.C", "svg:rect", "my-element", "Foo", "$x", "_y"] {
        let source = format!("<{name}/>");
        assert_eq!(parse(&source).unwrap().name.unwrap().text, name);
    }
}

#[test]
fn attribute_values_may_be_elements_and_strings_keep_their_text() {
    let element = parse("<a b=<c/> d='it\\\"s' />").unwrap();
    assert!(matches!(
        &element.attributes[0],
        JsxAttribute::Named {
            value: Some(JsxValue::Element(_)),
            ..
        }
    ));
    let JsxAttribute::Named {
        value: Some(JsxValue::String { start, end }),
        ..
    } = &element.attributes[1]
    else {
        panic!("expected a string value");
    };
    assert_eq!(&"<a b=<c/> d='it\\\"s' />"[*start..*end], "'it\\\"s'");
}

#[test]
fn comments_and_newlines_are_allowed_inside_a_tag() {
    assert!(parse("<a /* c */ b=\"1\" // d\n c=\"2\"\n/>").is_ok());
}

#[test]
fn mismatched_unterminated_and_malformed_elements_fail_with_late_errors() {
    for source in [
        "<a></b>",
        "<a>",
        "<a b=>",
        "<a>}</a>",
        "<a>></a>",
        "<a {x}/>",
        "<a b={}/>",
        "<a></a",
    ] {
        let error = parse(source).unwrap_err();
        assert!(error.late, "{source}: {error:?}");
    }
    // Not a tag at all: an early failure, which the lexer may read as punctuation.
    let error = parse("< 1>").unwrap_err();
    assert!(!error.late);
}

#[test]
fn empty_expressions_are_trivia_only() {
    assert!(is_empty_expression(""));
    assert!(is_empty_expression(" /* c */ // d\n "));
    assert!(!is_empty_expression("x"));
    assert!(!is_empty_expression("/* unterminated"));
}

#[test]
fn text_follows_typescripts_trimming_and_joining() {
    assert_eq!(text_value("hello"), Some("hello".to_string()));
    assert_eq!(text_value("  hello  "), Some("  hello  ".to_string()));
    assert_eq!(text_value("\n   a\n   b   \n"), Some("a b".to_string()));
    assert_eq!(
        text_value("first  \n  second"),
        Some("first second".to_string())
    );
    assert_eq!(text_value("\n   \n"), None);
    assert_eq!(text_value("a &amp; b"), Some("a & b".to_string()));
    assert!(is_blank_with_newline("\n  "));
    assert!(!is_blank_with_newline("  "));
    assert!(!is_blank_with_newline(" x\n"));
}

#[test]
fn entities_decode_by_name_decimal_and_hex_and_unknown_ones_stay() {
    assert_eq!(decode_entities("&lt;&gt;&amp;&quot;&nbsp;"), "<>&\"\u{a0}");
    assert_eq!(decode_entities("&#65;&#x42;&#X43;"), "ABC");
    assert_eq!(decode_entities("&unknown; &; &amp"), "&unknown; &; &amp");
    assert_eq!(decode_entities("a & b"), "a & b");
    assert_eq!(decode_entities("&hearts;"), "\u{2665}");
}

#[test]
fn intrinsic_names_follow_typescripts_rule() {
    for name in ["div", "a", "my-element", "svg:rect"] {
        assert!(is_intrinsic_name(name), "{name}");
    }
    for name in ["Foo", "A", "_x", "$y", "a.b", "Foo.Bar"] {
        assert!(!is_intrinsic_name(name), "{name}");
    }
}

#[test]
fn nesting_depth_is_bounded() {
    let source = format!("{}{}", "<a>".repeat(200), "</a>".repeat(200));
    let error = parse(&source).unwrap_err();
    assert!(error.message.contains("too deep"), "{error:?}");
}

#[test]
fn pragmas_are_read_from_the_leading_comments_only() {
    let pragmas = Pragmas::of(
        "/** @jsx h\n * @jsxFrag Frag */\n// @jsxImportSource preact\n/* @jsxRuntime automatic */\nconst a = 1; /** @jsx late */",
    );
    assert_eq!(pragmas.factory.as_deref(), Some("h"));
    assert_eq!(pragmas.fragment.as_deref(), Some("Frag"));
    assert_eq!(pragmas.import_source.as_deref(), Some("preact"));
    assert_eq!(pragmas.runtime.as_deref(), Some("automatic"));
    assert_eq!(
        Pragmas::of("const a = 1;\n/** @jsx h */"),
        Pragmas::default()
    );
}
