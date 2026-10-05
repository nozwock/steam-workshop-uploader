use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Render GitHub Flavored Markdown to Steam's markup format.
///
/// See https://steamcommunity.com/comment/Recommendation/formattinghelp.
pub fn markdown_to_steam(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut renderer = SteamRenderer::new();
    renderer.render(parser)
}

struct SteamRenderer {
    output: String,
    list_stack: usize,
    in_table_head: bool,
    in_code_block: bool,
    in_block_quote: bool,
    in_image: Option<String>,
    in_heading_level: Option<u8>,
}

impl SteamRenderer {
    fn new() -> Self {
        Self {
            output: String::new(),
            list_stack: 0,
            in_table_head: false,
            in_code_block: false,
            in_block_quote: false,
            in_image: None,
            in_heading_level: None,
        }
    }

    fn ensure_newline(&mut self) {
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn ensure_double_newline(&mut self) {
        if self.output.is_empty() {
            return;
        }
        if self.output.ends_with("\n\n") {
            return;
        }
        if self.output.ends_with('\n') {
            self.output.push('\n');
        } else {
            self.output.push_str("\n\n");
        }
    }

    fn render<'a, I>(&mut self, events: I) -> String
    where
        I: IntoIterator<Item = Event<'a>>,
    {
        for event in events {
            self.handle_event(event);
        }

        self.output.trim().to_string()
    }

    fn handle_event<'a>(&mut self, event: Event<'a>) {
        // If we are inside an image tag, suppress inner text (alt text)
        if self.in_image.is_some() {
            if let Event::End(TagEnd::Image) = event {
                let url = self.in_image.take().unwrap_or_default();
                self.output.push_str(&format!("[img]{url}[/img]"));
            }
            return;
        }

        match event {
            Event::Start(tag) => self.handle_start_tag(tag),
            Event::End(tag_end) => self.handle_end_tag(tag_end),
            Event::Text(text) => {
                self.output.push_str(&text);
            }
            Event::Code(code) => {
                self.output
                    .push_str(&format!("[noparse]`{code}`[/noparse]"));
            }
            Event::InlineHtml(raw) | Event::Html(raw) => {
                self.handle_html(&raw);
            }
            Event::SoftBreak => {
                if self.in_code_block {
                    self.output.push('\n');
                } else if !self.output.ends_with([' ', '\n']) {
                    self.output.push(' ');
                }
            }
            Event::HardBreak => {
                self.output.push('\n');
            }
            Event::Rule => {
                self.ensure_double_newline();
                self.output.push_str("[hr][/hr]\n\n");
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    self.output.push_str("[X] ");
                } else {
                    self.output.push_str("[ ] ");
                }
            }
            Event::FootnoteReference(name) => {
                self.output.push_str(&format!("[{name}]"));
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                self.output.push_str(&math);
            }
        }
    }

    fn handle_start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if self.in_block_quote {
                    self.in_block_quote = false;
                } else if self.list_stack == 0 {
                    self.ensure_double_newline();
                }
            }
            Tag::Heading { level, .. } => {
                self.ensure_newline();
                let lvl = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3, // Clamp H4..H6 to H3
                };
                self.in_heading_level = Some(lvl);
                self.output.push_str(&format!("[h{lvl}]"));
            }
            Tag::BlockQuote(_) => {
                self.ensure_double_newline();
                self.in_block_quote = true;
                self.output.push_str("[quote]\n");
            }
            Tag::CodeBlock(_) => {
                self.ensure_double_newline();
                self.in_code_block = true;
                self.output.push_str("[code]\n");
            }
            Tag::List(start_num) => {
                self.ensure_newline();
                let is_ordered = start_num.is_some();
                self.list_stack = self.list_stack.strict_add(1); // Panic on overflow
                if is_ordered {
                    self.output.push_str("[olist]\n");
                } else {
                    self.output.push_str("[list]\n");
                }
            }
            Tag::Item => {
                self.ensure_newline();
                self.output.push_str("[*]");
            }
            Tag::Emphasis => {
                self.output.push_str("[i]");
            }
            Tag::Strong => {
                self.output.push_str("[b]");
            }
            Tag::Strikethrough => {
                self.output.push_str("[strike]");
            }
            Tag::Link { dest_url, .. } => {
                self.output.push_str(&format!("[url={dest_url}]"));
            }
            Tag::Image { dest_url, .. } => {
                self.in_image = Some(dest_url.to_string());
            }
            Tag::Table(_) => {
                self.ensure_double_newline();
                self.output.push_str("[table]\n");
            }
            Tag::TableHead => {
                self.in_table_head = true;
                self.ensure_newline();
                self.output.push_str("[tr]");
            }
            Tag::TableRow => {
                self.ensure_newline();
                self.output.push_str("[tr]");
            }
            Tag::TableCell => {
                if self.in_table_head {
                    self.output.push_str("[th]");
                } else {
                    self.output.push_str("[td]");
                }
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => {
                if self.list_stack == 0 {
                    self.output.push_str("\n\n");
                } else {
                    self.output.push('\n');
                }
            }
            TagEnd::Heading(_) => {
                if let Some(lvl) = self.in_heading_level.take() {
                    self.output.push_str(&format!("[/h{lvl}]\n"));
                }
            }
            TagEnd::BlockQuote(_) => {
                if self.output.ends_with("\n\n") {
                    self.output.pop();
                }
                self.ensure_newline();
                self.output.push_str("[/quote]\n\n");
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.ensure_newline();
                self.output.push_str("[/code]\n\n");
            }
            TagEnd::List(is_ordered) => {
                self.list_stack = self.list_stack.saturating_sub(1);
                self.ensure_newline();
                if is_ordered {
                    self.output.push_str("[/olist]\n");
                } else {
                    self.output.push_str("[/list]\n");
                }
                if self.list_stack == 0 {
                    self.output.push('\n');
                }
            }
            TagEnd::Item => {
                self.ensure_newline();
            }
            TagEnd::Emphasis => {
                self.output.push_str("[/i]");
            }
            TagEnd::Strong => {
                self.output.push_str("[/b]");
            }
            TagEnd::Strikethrough => {
                self.output.push_str("[/strike]");
            }
            TagEnd::Link => {
                self.output.push_str("[/url]");
            }
            TagEnd::Image => {
                let url = self.in_image.take().unwrap_or_default();
                self.output.push_str(&format!("[img]{url}[/img]"));
            }
            TagEnd::Table => {
                self.ensure_newline();
                self.output.push_str("[/table]\n\n");
            }
            TagEnd::TableHead => {
                self.in_table_head = false;
                self.output.push_str("[/tr]\n");
            }
            TagEnd::TableRow => {
                self.output.push_str("[/tr]\n");
            }
            TagEnd::TableCell => {
                if self.in_table_head {
                    self.output.push_str("[/th]");
                } else {
                    self.output.push_str("[/td]");
                }
            }
            _ => {}
        }
    }

    fn handle_html(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("<spoiler>") {
            self.output.push_str("[spoiler]");
        } else if trimmed.eq_ignore_ascii_case("</spoiler>") {
            self.output.push_str("[/spoiler]");
        } else if trimmed.eq_ignore_ascii_case("<u>") {
            self.output.push_str("[u]");
        } else if trimmed.eq_ignore_ascii_case("</u>") {
            self.output.push_str("[/u]");
        } else if trimmed.eq_ignore_ascii_case("<br>")
            || trimmed.eq_ignore_ascii_case("<br/>")
            || trimmed.eq_ignore_ascii_case("<br />")
        {
            self.output.push('\n');
        } else {
            self.output.push_str(raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_break_line_wrapping() {
        let md = "\
Mandaremus, 
fore ut hic noster.

Ipsa declinatio ad
libidinem fingitur.";

        let expected = "\
Mandaremus, fore ut hic noster.

Ipsa declinatio ad libidinem fingitur.";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_hard_break() {
        let md = "Line one  \nLine two\\\nLine three";
        let expected = "Line one\nLine two\nLine three";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_headings_and_clamping() {
        let md = "\
# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6";

        let expected = "\
[h1]Heading 1[/h1]
[h2]Heading 2[/h2]
[h3]Heading 3[/h3]
[h3]Heading 4[/h3]
[h3]Heading 5[/h3]
[h3]Heading 6[/h3]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_text_decorations() {
        let md = "This is **bold**, *italic*, and ~~strikethrough~~ text. Also <u>underlined</u>.";
        let expected = "This is [b]bold[/b], [i]italic[/i], and [strike]strikethrough[/strike] text. Also [u]underlined[/u].";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_nested_decorations() {
        let md = "This is ***bold and italic***.";
        let result = markdown_to_steam(md);
        assert!(
            result == "This is [b][i]bold and italic[/i][/b]."
                || result == "This is [i][b]bold and italic[/b][/i]."
        );
    }

    #[test]
    fn test_inline_code() {
        let md = "Install via `cargo install crate-name`.";
        let expected = "Install via [noparse]`cargo install crate-name`[/noparse].";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_code_block() {
        let md = "\
```rust
fn main() {
    println!(\"Hello, world!\");
}
```";

        let expected = "\
[code]
fn main() {
    println!(\"Hello, world!\");
}
[/code]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_unordered_list() {
        let md = "\
- First item
- Second item
- Third item";

        let expected = "\
[list]
[*]First item
[*]Second item
[*]Third item
[/list]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_ordered_list() {
        let md = "\
1. Step one
2. Step two
3. Step three";

        let expected = "\
[olist]
[*]Step one
[*]Step two
[*]Step three
[/olist]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_task_list() {
        let md = "\
- [ ] Todo item
- [x] Done item";

        let expected = "\
[list]
[*][ ] Todo item
[*][X] Done item
[/list]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_nested_lists() {
        let md = "\
- Parent 1
  - Child 1
  - Child 2
- Parent 2";

        let expected = "\
[list]
[*]Parent 1
[list]
[*]Child 1
[*]Child 2
[/list]
[*]Parent 2
[/list]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_blockquote() {
        let md = "> This is a quote.\n> Spanning multiple lines.";
        let expected = "[quote]\nThis is a quote. Spanning multiple lines.\n[/quote]";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_horizontal_rule() {
        let md = "Above\n\n---\n\nBelow";
        let expected = "Above\n\n[hr][/hr]\n\nBelow";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_links_and_images() {
        let md =
            "Visit [GitHub](https://github.com) or check ![Preview](https://example.com/logo.png).";
        let expected = "Visit [url=https://github.com]GitHub[/url] or check [img]https://example.com/logo.png[/img].";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_autolink() {
        let md = "Check out <https://store.steampowered.com>.";
        let expected =
            "Check out [url=https://store.steampowered.com]https://store.steampowered.com[/url].";
        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_tables() {
        let md = "\
| Header A | Header B |
| :--- | :--- |
| Cell 1 | Cell 2 |
| Cell 3 | Cell 4 |";

        let expected = "\
[table]
[tr][th]Header A[/th][th]Header B[/th][/tr]
[tr][td]Cell 1[/td][td]Cell 2[/td][/tr]
[tr][td]Cell 3[/td][td]Cell 4[/td][/tr]
[/table]";

        assert_eq!(markdown_to_steam(md), expected);
    }

    #[test]
    fn test_spoiler_tag() {
        let md = "The secret code is <spoiler>**12345**</spoiler>.";
        let expected = "The secret code is [spoiler][b]12345[/b][/spoiler].";
        assert_eq!(markdown_to_steam(md), expected);
    }
}
