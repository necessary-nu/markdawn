// Can't access private functions, so replicate the parse_html_open_tag_prefix logic
fn parse_html_tag_name_prefix(line: &str) -> Option<(&str, usize)> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() { return None; }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            end = index + ch.len_utf8();
        } else { break; }
    }
    Some((&line[..end], end))
}

fn parse_html_attribute_name_prefix(line: &str) -> Option<usize> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || matches!(first, '_' | ':')) { return None; }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-') {
            end = index + ch.len_utf8();
        } else { break; }
    }
    Some(end)
}

fn is_html_whitespace(ch: char) -> bool { matches!(ch, ' ' | '\t' | '\n' | '\r') }
fn count_html_whitespace(s: &str) -> usize {
    s.chars().take_while(|c| is_html_whitespace(*c)).count()
}

fn parse_html_attribute_value_prefix(line: &str) -> Option<usize> {
    let quote = line.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = line[1..].find(quote)? + 2;
        return Some(end);
    }
    let mut end = 0;
    for (index, ch) in line.char_indices() {
        if is_html_whitespace(ch) || matches!(ch, '"' | '\'' | '=' | '<' | '>' | '`') {
            break;
        }
        end = index + ch.len_utf8();
    }
    (end > 0).then_some(end)
}

fn parse_html_open_tag_prefix(line: &str) -> Option<(&str, usize)> {
    let rest = line.strip_prefix('<')?;
    if rest.starts_with('/') { return None; }
    let (name, name_len) = parse_html_tag_name_prefix(rest)?;
    let mut consumed = 1 + name_len;
    loop {
        let rest = &line[consumed..];
        eprintln!("  loop: consumed={}, rest={:?}", consumed, rest);
        if rest.starts_with('>') { return Some((name, consumed + 1)); }
        if rest.starts_with("/>") { return Some((name, consumed + 2)); }
        let whitespace = count_html_whitespace(rest);
        if whitespace == 0 { eprintln!("  NO WS"); return None; }
        consumed += whitespace;
        let rest = &line[consumed..];
        if rest.starts_with('>') { return Some((name, consumed + 1)); }
        if rest.starts_with("/>") { return Some((name, consumed + 2)); }
        consumed += parse_html_attribute_name_prefix(rest)?;
        consumed += count_html_whitespace(&line[consumed..]);
        if line[consumed..].starts_with('=') {
            consumed += 1;
            consumed += count_html_whitespace(&line[consumed..]);
            consumed += parse_html_attribute_value_prefix(&line[consumed..])?;
        }
    }
}

fn main() {
    let input = "<a foo=\"bar\" bam='baz' _boolean zoop:33=zoop:33 />";
    eprintln!("input: {:?}", input);
    let result = parse_html_open_tag_prefix(input);
    eprintln!("result: {:?}", result);
}
