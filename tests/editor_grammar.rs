use serde_json::Value;
use std::fs;
use std::path::Path;

const SOURCE_GRAMMAR: &str = "src/traqula.pest";
const EDITOR_GRAMMAR: &str = "traqula-vscode/syntaxes/traqula.tmLanguage.json";
const PACKAGED_EDITOR_GRAMMAR: &str = "traqula-vscode/extension/syntaxes/traqula.tmLanguage.json";

fn read(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("could not read {relative}: {error}"))
}

fn quoted_rule_tokens(source: &str, rule: &str) -> Vec<String> {
    let prefix = format!("{rule} =");
    let line = source
        .lines()
        .find(|line| line.trim_start().starts_with(&prefix))
        .unwrap_or_else(|| panic!("missing {rule} rule"));
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn regex_escape(token: &str) -> String {
    let mut escaped = String::new();
    for character in token.chars() {
        if matches!(
            character,
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn grammar_match<'a>(grammar: &'a Value, group: &str) -> &'a str {
    grammar["repository"][group]["patterns"]
        .as_array()
        .and_then(|patterns| patterns.first())
        .unwrap_or(&grammar["repository"][group])["match"]
        .as_str()
        .unwrap_or_else(|| panic!("missing match expression for {group}"))
}

#[test]
fn packaged_editor_grammar_is_an_exact_copy() {
    assert_eq!(read(EDITOR_GRAMMAR), read(PACKAGED_EDITOR_GRAMMAR));
}

#[test]
fn editor_keywords_and_comparators_follow_the_parser_grammar() {
    let parser = read(SOURCE_GRAMMAR);
    let editor: Value = serde_json::from_str(&read(EDITOR_GRAMMAR)).unwrap();

    let keywords = quoted_rule_tokens(&parser, "keyword");
    let expected_keywords = format!(r"\b({})\b", keywords.join("|"));
    assert_eq!(grammar_match(&editor, "keywords"), expected_keywords);

    let mut operators: Vec<String> = quoted_rule_tokens(&parser, "comparator")
        .iter()
        .map(|token| regex_escape(token))
        .collect();
    operators.extend([r"\+".to_owned(), r"\|".to_owned()]);
    let expected_operators = format!("({})", operators.join("|"));
    assert_eq!(grammar_match(&editor, "operators"), expected_operators);
}
