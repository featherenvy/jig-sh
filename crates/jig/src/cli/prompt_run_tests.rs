use std::io::Cursor;

use super::super::prompt::PromptAddOpts;
use super::*;

#[test]
fn interactive_prompt_add_collects_missing_fields() {
    let opts = PromptAddOpts {
        name: None,
        body: None,
        file: None,
        no_editor: false,
        description: None,
        tags: Vec::new(),
    };
    let input = Cursor::new("review-loop\nReview loop\nreview, codex\nLine one\nLine two\n.\n");
    let mut output = Vec::new();

    let request = interactive_prompt_add_request(opts, input, &mut output).unwrap();

    assert_eq!(request.name, "review-loop");
    assert_eq!(request.description.as_deref(), Some("Review loop"));
    assert_eq!(request.tags, vec!["review", "codex"]);
    assert_eq!(request.body.as_deref(), Some("Line one\nLine two"));
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("Interactive prompt add")
    );
}

#[test]
fn interactive_prompt_add_rejects_eof_before_required_name() {
    let opts = PromptAddOpts {
        name: None,
        body: None,
        file: None,
        no_editor: false,
        description: None,
        tags: Vec::new(),
    };
    let input = Cursor::new("");
    let mut output = Vec::new();

    let error = interactive_prompt_add_request(opts, input, &mut output)
        .unwrap_err()
        .to_string();

    assert!(error.contains("interactive prompt add ended before prompt name was complete"));
    assert!(String::from_utf8(output).unwrap().contains("Prompt name:"));
}

#[test]
fn interactive_prompt_add_uses_supplied_name_and_metadata() {
    let opts = PromptAddOpts {
        name: Some("review-loop".into()),
        body: None,
        file: None,
        no_editor: true,
        description: Some("Review loop".into()),
        tags: vec!["review".into()],
    };
    let input = Cursor::new("Body\n.\n");
    let mut output = Vec::new();

    let request = interactive_prompt_add_request(opts, input, &mut output).unwrap();

    assert_eq!(request.name, "review-loop");
    assert_eq!(request.description.as_deref(), Some("Review loop"));
    assert_eq!(request.tags, vec!["review"]);
    assert_eq!(request.body.as_deref(), Some("Body"));
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("Prompt name:"));
    assert!(!output.contains("Description"));
}

#[test]
fn interactive_prompt_add_accepts_eof_after_body_content() {
    let opts = PromptAddOpts {
        name: Some("review-loop".into()),
        body: None,
        file: None,
        no_editor: true,
        description: None,
        tags: Vec::new(),
    };
    let input = Cursor::new("\n\nBody\n");
    let mut output = Vec::new();

    let request = interactive_prompt_add_request(opts, input, &mut output).unwrap();

    assert_eq!(request.body.as_deref(), Some("Body"));
}

#[test]
fn interactive_prompt_add_rejects_empty_body() {
    let opts = PromptAddOpts {
        name: Some("review-loop".into()),
        body: None,
        file: None,
        no_editor: true,
        description: None,
        tags: Vec::new(),
    };
    let input = Cursor::new("\n\n.\n");
    let mut output = Vec::new();

    let error = interactive_prompt_add_request(opts, input, &mut output)
        .unwrap_err()
        .to_string();

    assert!(error.contains("prompt body cannot be empty"));
}
