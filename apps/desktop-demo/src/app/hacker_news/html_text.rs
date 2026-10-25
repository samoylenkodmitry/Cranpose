pub(super) fn html_to_plain_text(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut tag = String::new();
    let mut entity = String::new();
    let mut in_tag = false;
    let mut in_entity = false;

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                append_tag_separator(&tag, &mut plain);
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }

        if in_entity {
            entity.push(ch);
            if ch == ';' {
                plain.push_str(decode_html_entity(&entity));
                entity.clear();
                in_entity = false;
            }
            continue;
        }

        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '&' => {
                in_entity = true;
                entity.clear();
                entity.push(ch);
            }
            _ => plain.push(ch),
        }
    }

    if in_entity {
        plain.push_str(&entity);
    }

    collapse_text_whitespace(&plain)
}

fn append_tag_separator(tag: &str, plain: &mut String) {
    let normalized = tag.trim().to_ascii_lowercase();
    if (normalized.starts_with("br")
        || normalized.starts_with("p")
        || normalized.starts_with("/p")
        || normalized.starts_with("div")
        || normalized.starts_with("/div")
        || normalized.starts_with("pre")
        || normalized.starts_with("/pre"))
        && !plain.ends_with('\n')
        && !plain.is_empty()
    {
        plain.push('\n');
    }
    if normalized.starts_with("li") {
        if !plain.ends_with('\n') && !plain.is_empty() {
            plain.push('\n');
        }
        plain.push_str("• ");
    }
}

fn decode_html_entity(entity: &str) -> &'static str {
    match entity {
        "&amp;" => "&",
        "&gt;" => ">",
        "&lt;" => "<",
        "&quot;" => "\"",
        "&#39;" | "&#x27;" => "'",
        "&#47;" | "&#x2F;" => "/",
        "&nbsp;" => " ",
        _ => "",
    }
}

fn collapse_text_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut newline_run = 0usize;

    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                pending_space = false;
                if newline_run < 2 {
                    collapsed.push('\n');
                }
                newline_run += 1;
            }
            c if c.is_whitespace() => {
                if !collapsed.ends_with('\n') {
                    pending_space = true;
                }
            }
            c => {
                if pending_space && !collapsed.is_empty() && !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
                pending_space = false;
                newline_run = 0;
                collapsed.push(c);
            }
        }
    }

    collapsed.trim().to_string()
}
