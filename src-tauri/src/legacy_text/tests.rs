use super::*;

#[test]
fn legacy_framing_is_projected_to_the_users_words() {
    let framed = "[[type:inspiration]]\n<type-context>\nBoard.\n</type-context>\nmake it warmer";
    let bare = "[[type:inspiration]]\nmake it warmer";
    for content in [framed, bare] {
        let mut message = json!({"role": "user", "content": content});
        project_user_message(&mut message);
        assert_eq!(message["content"], "make it warmer", "{content}");
    }
    let mut parts = json!({"role": "user", "content": [
        {"type": "image_url", "image_url": {"url": "x"}},
        {"type": "text", "text": framed}
    ]});
    project_user_message(&mut parts);
    assert_eq!(parts["content"][1]["text"], "make it warmer");
}

#[test]
fn malformed_prefixes_in_older_messages_are_not_framing() {
    for text in [
        "[[type:Bad Id]]\nliteral",
        "[[type:]]\nliteral",
        "[[type:notes\nliteral",
        "[[type:notes]]\n<type-context>\nunclosed",
        "[[type:notes]] inline words, not a tag line",
        "[[type:notes]]keep",
        "[[type:notes]]\n<type-context>\nbriefing</type-context> trailing",
        "see [[type:notes]]\nliteral",
    ] {
        let mut message = json!({"role": "user", "content": text});
        project_user_message(&mut message);
        assert_eq!(message["content"], text, "{text}");
    }
}

#[test]
fn a_message_the_engine_marked_structured_is_never_projected() {
    let literal = "[[type:notes]]\n<type-context>\nlooks like framing</type-context>\nliteral";
    let mut message = json!({"role": "user", "content": literal, "dial_structured_text": true});
    project_user_message(&mut message);
    assert_eq!(message["content"], literal);
}

#[test]
fn only_user_messages_are_projected() {
    let reply = "[[type:notes]]\nreply";
    let mut assistant = json!({"role": "assistant", "content": reply});
    project_user_message(&mut assistant);
    assert_eq!(assistant["content"], reply);
}
