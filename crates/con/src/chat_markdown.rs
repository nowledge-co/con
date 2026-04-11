use gpui::{
    AbsoluteLength, AnyElement, DefiniteLength, FontStyle, FontWeight, Hsla, IntoElement,
    ParentElement, SharedString, Styled, StyledText, TextStyle, UnderlineStyle, WhiteSpace, div,
    px,
};
use gpui_component::Theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMarkdownTone {
    Message,
    Thinking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownBlock {
    Paragraph(Vec<MarkdownInline>),
    Heading {
        level: u8,
        inlines: Vec<MarkdownInline>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    BlockQuote(Vec<MarkdownBlock>),
    List {
        ordered: bool,
        start: usize,
        items: Vec<Vec<MarkdownBlock>>,
    },
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownInline {
    Text(String),
    Code(String),
    Emphasis(Vec<MarkdownInline>),
    Strong(Vec<MarkdownInline>),
    Strikethrough(Vec<MarkdownInline>),
    Link {
        label: Vec<MarkdownInline>,
        destination: String,
    },
    SoftBreak,
    LineBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockEnd {
    BlockQuote,
    Item,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineEnd {
    Paragraph,
    Heading,
    Emphasis,
    Strong,
    Strikethrough,
    Link,
}

struct ChatMarkdownStyle<'a> {
    theme: &'a Theme,
    tone: ChatMarkdownTone,
    content_width: gpui::Pixels,
    base_font_size: gpui::Pixels,
    base_line_height: gpui::Pixels,
    code_font_size: gpui::Pixels,
    code_line_height: gpui::Pixels,
    text_color: Hsla,
    muted_text_color: Hsla,
    inline_code_background: Hsla,
    inline_code_text_color: Hsla,
    code_block_background: Hsla,
    code_block_language_background: Hsla,
    code_block_language_text_color: Hsla,
    quote_background: Hsla,
    quote_tint: Hsla,
    rule_color: Hsla,
    link_color: Hsla,
    block_gap: gpui::Pixels,
    inner_gap: gpui::Pixels,
    inline_code_padding_x: gpui::Pixels,
    inline_code_padding_y: gpui::Pixels,
    inline_code_radius: gpui::Pixels,
    code_block_radius: gpui::Pixels,
    code_block_language_radius: gpui::Pixels,
}

impl<'a> ChatMarkdownStyle<'a> {
    fn new(theme: &'a Theme, tone: ChatMarkdownTone) -> Self {
        match tone {
            ChatMarkdownTone::Message => Self {
                theme,
                tone,
                content_width: px(720.0),
                base_font_size: px(14.5),
                base_line_height: px(23.0),
                code_font_size: px(13.0),
                code_line_height: px(21.0),
                text_color: theme.foreground.opacity(0.88),
                muted_text_color: theme.muted_foreground.opacity(0.74),
                inline_code_background: theme.secondary_active.opacity(0.82),
                inline_code_text_color: theme.foreground.opacity(0.92),
                code_block_background: theme.secondary.opacity(0.92),
                code_block_language_background: theme.secondary_hover.opacity(0.96),
                code_block_language_text_color: theme.muted_foreground.opacity(0.78),
                quote_background: theme.secondary.opacity(0.84),
                quote_tint: theme.primary.opacity(0.42),
                rule_color: theme.muted_foreground.opacity(0.16),
                link_color: theme.primary,
                block_gap: px(13.0),
                inner_gap: px(9.0),
                inline_code_padding_x: px(6.0),
                inline_code_padding_y: px(2.0),
                inline_code_radius: px(6.0),
                code_block_radius: px(9.0),
                code_block_language_radius: px(5.0),
            },
            ChatMarkdownTone::Thinking => Self {
                theme,
                tone,
                content_width: px(640.0),
                base_font_size: px(12.5),
                base_line_height: px(19.5),
                code_font_size: px(12.0),
                code_line_height: px(18.0),
                text_color: theme.muted_foreground.opacity(0.66),
                muted_text_color: theme.muted_foreground.opacity(0.58),
                inline_code_background: theme.secondary_active.opacity(0.78),
                inline_code_text_color: theme.foreground.opacity(0.78),
                code_block_background: theme.secondary.opacity(0.72),
                code_block_language_background: theme.secondary_hover.opacity(0.88),
                code_block_language_text_color: theme.muted_foreground.opacity(0.66),
                quote_background: theme.secondary.opacity(0.58),
                quote_tint: theme.primary.opacity(0.30),
                rule_color: theme.muted_foreground.opacity(0.12),
                link_color: theme.primary.opacity(0.82),
                block_gap: px(10.0),
                inner_gap: px(8.0),
                inline_code_padding_x: px(5.0),
                inline_code_padding_y: px(1.0),
                inline_code_radius: px(5.0),
                code_block_radius: px(8.0),
                code_block_language_radius: px(4.0),
            },
        }
    }

    fn base_text_style(&self) -> TextStyle {
        TextStyle {
            color: self.text_color,
            font_family: self.theme.font_family.clone(),
            font_size: self.base_font_size.into(),
            line_height: self.base_line_height.into(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            white_space: WhiteSpace::Normal,
            ..Default::default()
        }
    }

    fn heading_text_style(&self, level: u8) -> TextStyle {
        let (font_size, line_height, weight) = match (self.tone, level) {
            (ChatMarkdownTone::Message, 1) => (px(19.0), px(27.0), FontWeight::BOLD),
            (ChatMarkdownTone::Message, 2) => (px(17.0), px(25.0), FontWeight::SEMIBOLD),
            (ChatMarkdownTone::Message, 3) => (px(15.5), px(23.0), FontWeight::SEMIBOLD),
            (ChatMarkdownTone::Thinking, 1) => (px(14.5), px(21.0), FontWeight::SEMIBOLD),
            (ChatMarkdownTone::Thinking, 2) => (px(13.5), px(20.0), FontWeight::SEMIBOLD),
            (ChatMarkdownTone::Thinking, _) => (px(12.75), px(19.0), FontWeight::MEDIUM),
            (_, _) => (
                self.base_font_size,
                self.base_line_height,
                FontWeight::MEDIUM,
            ),
        };

        TextStyle {
            font_size: font_size.into(),
            line_height: line_height.into(),
            font_weight: weight,
            ..self.base_text_style()
        }
    }

    fn code_text_style(&self) -> TextStyle {
        TextStyle {
            color: self.text_color,
            font_family: self.theme.mono_font_family.clone(),
            font_size: self.code_font_size.into(),
            line_height: self.code_line_height.into(),
            white_space: WhiteSpace::Normal,
            ..Default::default()
        }
    }
}

pub fn render_chat_markdown(source: &str, tone: ChatMarkdownTone, theme: &Theme) -> AnyElement {
    let style = ChatMarkdownStyle::new(theme, tone);
    let blocks = parse_markdown(source);

    if blocks.is_empty() {
        return div().into_any_element();
    }

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(style.block_gap)
        .max_w(style.content_width)
        .children(
            blocks
                .iter()
                .enumerate()
                .map(|(idx, block)| render_block(block, idx, &style)),
        )
        .into_any_element()
}

fn parse_markdown(source: &str) -> Vec<MarkdownBlock> {
    let parser = Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES,
    );
    let mut events = parser.peekable();
    parse_blocks(&mut events, None)
}

fn parse_blocks<'a>(
    events: &mut std::iter::Peekable<Parser<'a>>,
    until: Option<BlockEnd>,
) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();

    while let Some(event) = events.next() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    blocks.push(MarkdownBlock::Paragraph(parse_inlines(
                        events,
                        InlineEnd::Paragraph,
                    )));
                }
                Tag::Heading { level, .. } => {
                    blocks.push(MarkdownBlock::Heading {
                        level: heading_level_to_u8(level),
                        inlines: parse_inlines(events, InlineEnd::Heading),
                    });
                }
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Indented => None,
                        CodeBlockKind::Fenced(lang) => {
                            let lang = lang.trim().to_string();
                            (!lang.is_empty()).then_some(lang)
                        }
                    };

                    let mut code = String::new();
                    while let Some(inner) = events.next() {
                        match inner {
                            Event::End(TagEnd::CodeBlock) => break,
                            Event::Text(text) | Event::Code(text) | Event::Html(text) => {
                                code.push_str(text.as_ref())
                            }
                            Event::SoftBreak | Event::HardBreak => code.push('\n'),
                            _ => {}
                        }
                    }

                    blocks.push(MarkdownBlock::CodeBlock { language, code });
                }
                Tag::BlockQuote(_) => {
                    blocks.push(MarkdownBlock::BlockQuote(parse_blocks(
                        events,
                        Some(BlockEnd::BlockQuote),
                    )));
                }
                Tag::List(start) => {
                    blocks.push(parse_list(events, start.map(|value| value as usize)));
                }
                Tag::Item => {
                    blocks.extend(parse_blocks(events, Some(BlockEnd::Item)));
                }
                _ => {}
            },
            Event::End(end) => {
                if block_end_matches(&end, until) {
                    break;
                }
            }
            Event::Rule => blocks.push(MarkdownBlock::Rule),
            Event::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    blocks.push(MarkdownBlock::Paragraph(vec![MarkdownInline::Text(
                        trimmed.to_string(),
                    )]));
                }
            }
            Event::Code(text) => blocks.push(MarkdownBlock::Paragraph(vec![MarkdownInline::Code(
                text.to_string(),
            )])),
            Event::SoftBreak | Event::HardBreak => {}
            _ => {}
        }
    }

    blocks
}

fn parse_list<'a>(
    events: &mut std::iter::Peekable<Parser<'a>>,
    start: Option<usize>,
) -> MarkdownBlock {
    let ordered = start.is_some();
    let mut items = Vec::new();

    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Item) => {
                items.push(parse_blocks(events, Some(BlockEnd::Item)));
            }
            Event::End(end) if block_end_matches(&end, Some(BlockEnd::List)) => break,
            _ => {}
        }
    }

    MarkdownBlock::List {
        ordered,
        start: start.unwrap_or(1),
        items,
    }
}

fn parse_inlines<'a>(
    events: &mut std::iter::Peekable<Parser<'a>>,
    end: InlineEnd,
) -> Vec<MarkdownInline> {
    let mut inlines = Vec::new();

    while let Some(event) = events.next() {
        match event {
            Event::Text(text) => push_text(&mut inlines, text.as_ref()),
            Event::Code(text) => inlines.push(MarkdownInline::Code(text.to_string())),
            Event::SoftBreak => inlines.push(MarkdownInline::SoftBreak),
            Event::HardBreak => inlines.push(MarkdownInline::LineBreak),
            Event::TaskListMarker(checked) => {
                push_text(&mut inlines, if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(reference) => {
                push_text(&mut inlines, &format!("[{}]", reference));
            }
            Event::Html(text) | Event::InlineHtml(text) => push_text(&mut inlines, text.as_ref()),
            Event::Start(tag) => match tag {
                Tag::Emphasis => inlines.push(MarkdownInline::Emphasis(parse_inlines(
                    events,
                    InlineEnd::Emphasis,
                ))),
                Tag::Strong => inlines.push(MarkdownInline::Strong(parse_inlines(
                    events,
                    InlineEnd::Strong,
                ))),
                Tag::Strikethrough => inlines.push(MarkdownInline::Strikethrough(parse_inlines(
                    events,
                    InlineEnd::Strikethrough,
                ))),
                Tag::Link { dest_url, .. } => inlines.push(MarkdownInline::Link {
                    label: parse_inlines(events, InlineEnd::Link),
                    destination: dest_url.to_string(),
                }),
                Tag::Image { dest_url, .. } => {
                    let alt = parse_inlines(events, InlineEnd::Link);
                    let mut label = flatten_inlines_to_plain_text(&alt);
                    if label.is_empty() {
                        label = dest_url.to_string();
                    }
                    push_text(&mut inlines, &label);
                }
                _ => {}
            },
            Event::End(tag_end) => {
                if inline_end_matches(&tag_end, end) {
                    break;
                }
            }
            _ => {}
        }
    }

    coalesce_inlines(inlines)
}

fn push_text(inlines: &mut Vec<MarkdownInline>, text: &str) {
    if text.is_empty() {
        return;
    }

    if let Some(MarkdownInline::Text(existing)) = inlines.last_mut() {
        existing.push_str(text);
    } else {
        inlines.push(MarkdownInline::Text(text.to_string()));
    }
}

fn coalesce_inlines(inlines: Vec<MarkdownInline>) -> Vec<MarkdownInline> {
    let mut output = Vec::new();

    for inline in inlines {
        match inline {
            MarkdownInline::Text(text) if text.is_empty() => {}
            MarkdownInline::Text(text) => {
                if let Some(MarkdownInline::Text(existing)) = output.last_mut() {
                    existing.push_str(&text);
                } else {
                    output.push(MarkdownInline::Text(text));
                }
            }
            other => output.push(other),
        }
    }

    output
}

fn flatten_inlines_to_plain_text(inlines: &[MarkdownInline]) -> String {
    let mut text = String::new();

    for inline in inlines {
        match inline {
            MarkdownInline::Text(value) | MarkdownInline::Code(value) => text.push_str(value),
            MarkdownInline::Emphasis(children)
            | MarkdownInline::Strong(children)
            | MarkdownInline::Strikethrough(children) => {
                text.push_str(&flatten_inlines_to_plain_text(children));
            }
            MarkdownInline::Link { label, .. } => {
                text.push_str(&flatten_inlines_to_plain_text(label));
            }
            MarkdownInline::SoftBreak => text.push(' '),
            MarkdownInline::LineBreak => text.push('\n'),
        }
    }

    text
}

fn block_end_matches(end: &TagEnd, expected: Option<BlockEnd>) -> bool {
    match expected {
        Some(BlockEnd::BlockQuote) => matches!(end, TagEnd::BlockQuote(_)),
        Some(BlockEnd::Item) => matches!(end, TagEnd::Item),
        Some(BlockEnd::List) => matches!(end, TagEnd::List(_)),
        None => false,
    }
}

fn inline_end_matches(end: &TagEnd, expected: InlineEnd) -> bool {
    match expected {
        InlineEnd::Paragraph => matches!(end, TagEnd::Paragraph),
        InlineEnd::Heading => matches!(end, TagEnd::Heading(..)),
        InlineEnd::Emphasis => matches!(end, TagEnd::Emphasis),
        InlineEnd::Strong => matches!(end, TagEnd::Strong),
        InlineEnd::Strikethrough => matches!(end, TagEnd::Strikethrough),
        InlineEnd::Link => matches!(end, TagEnd::Link),
    }
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn render_block(block: &MarkdownBlock, index: usize, style: &ChatMarkdownStyle<'_>) -> AnyElement {
    match block {
        MarkdownBlock::Paragraph(inlines) => div()
            .w_full()
            .child(render_inline_content(
                inlines,
                &style.base_text_style(),
                style,
            ))
            .into_any_element(),
        MarkdownBlock::Heading { level, inlines } => div()
            .w_full()
            .pt(px(if *level <= 2 { 3.0 } else { 1.0 }))
            .child(render_inline_content(
                inlines,
                &style.heading_text_style(*level),
                style,
            ))
            .into_any_element(),
        MarkdownBlock::CodeBlock { language, code } => {
            render_code_block(index, language, code, style)
        }
        MarkdownBlock::BlockQuote(blocks) => div()
            .w_full()
            .px(px(10.0))
            .py(px(10.0))
            .rounded(px(8.0))
            .bg(style.quote_background)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(9.0))
                    .child(
                        div()
                            .w(px(3.0))
                            .h_full()
                            .min_h(px(18.0))
                            .bg(style.quote_tint),
                    )
                    .child(
                        div().flex().flex_col().gap(style.inner_gap).children(
                            blocks
                                .iter()
                                .enumerate()
                                .map(|(idx, block)| render_block(block, idx, style)),
                        ),
                    ),
            )
            .into_any_element(),
        MarkdownBlock::List {
            ordered,
            start,
            items,
        } => div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .children(items.iter().enumerate().map(|(item_idx, item_blocks)| {
                let marker = if *ordered {
                    format!("{}.", start + item_idx)
                } else {
                    "\u{2022}".to_string()
                };

                div()
                    .flex()
                    .items_start()
                    .gap(px(9.0))
                    .child(
                        div()
                            .pt(px(1.0))
                            .w(px(if *ordered { 28.0 } else { 14.0 }))
                            .text_right()
                            .font_family(style.theme.mono_font_family.clone())
                            .text_size(style.base_font_size)
                            .line_height(style.base_line_height)
                            .text_color(style.muted_text_color)
                            .child(marker),
                    )
                    .child(
                        div().flex().flex_col().gap(px(7.0)).flex_1().children(
                            item_blocks
                                .iter()
                                .enumerate()
                                .map(|(nested_idx, nested_block)| {
                                    render_block(nested_block, nested_idx, style)
                                }),
                        ),
                    )
                    .into_any_element()
            }))
            .into_any_element(),
        MarkdownBlock::Rule => div()
            .w_full()
            .h(px(1.0))
            .bg(style.rule_color)
            .into_any_element(),
    }
}

fn render_code_block(
    _index: usize,
    language: &Option<String>,
    code: &str,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let mono_style = style.code_text_style();
    let preserved_lines: Vec<String> = code.lines().map(preserve_code_indentation).collect();
    let has_trailing_newline = code.ends_with('\n');

    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .px(px(13.0))
        .py(px(12.0))
        .rounded(style.code_block_radius)
        .bg(style.code_block_background);

    if let Some(language) = language {
        block = block.child(
            div()
                .px(px(7.0))
                .py(px(3.0))
                .rounded(style.code_block_language_radius)
                .bg(style.code_block_language_background)
                .font_family(style.theme.mono_font_family.clone())
                .text_size(px(10.0))
                .line_height(px(12.0))
                .text_color(style.code_block_language_text_color)
                .child(language.clone()),
        );
    }

    let mut code_column = div().flex().flex_col().gap(px(2.0)).w_full();

    for (_line_idx, line) in preserved_lines.iter().enumerate() {
        code_column = code_column.child(
            div()
                .font_family(style.theme.mono_font_family.clone())
                .text_size(style.code_font_size)
                .line_height(style.code_line_height)
                .text_color(style.text_color)
                .child(
                    StyledText::new(line.clone()).with_runs(vec![mono_style.to_run(line.len())]),
                ),
        );
    }

    if preserved_lines.is_empty() || has_trailing_newline {
        code_column = code_column.child(
            div()
                .h(style.code_line_height)
                .font_family(style.theme.mono_font_family.clone()),
        );
    }

    block.child(code_column).into_any_element()
}

fn render_inline_text(
    inlines: &[MarkdownInline],
    base_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let (text, runs) = inline_runs(inlines, base_style, style);
    let font_size = text_style_font_size(base_style);
    let line_height = text_style_line_height(base_style, font_size);
    div()
        .w_full()
        .font_family(base_style.font_family.clone())
        .text_size(font_size)
        .line_height(line_height)
        .text_color(base_style.color)
        .child(StyledText::new(text).with_runs(runs))
        .into_any_element()
}

fn render_inline_content(
    inlines: &[MarkdownInline],
    base_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    if contains_inline_code(inlines) {
        render_inline_flow(inlines, base_style, style)
    } else {
        render_inline_text(inlines, base_style, style)
    }
}

fn contains_inline_code(inlines: &[MarkdownInline]) -> bool {
    inlines.iter().any(|inline| match inline {
        MarkdownInline::Code(_) => true,
        MarkdownInline::Emphasis(children)
        | MarkdownInline::Strong(children)
        | MarkdownInline::Strikethrough(children) => contains_inline_code(children),
        MarkdownInline::Link { label, .. } => contains_inline_code(label),
        MarkdownInline::Text(_) | MarkdownInline::SoftBreak | MarkdownInline::LineBreak => false,
    })
}

fn render_inline_flow(
    inlines: &[MarkdownInline],
    base_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let mut children = Vec::new();
    append_inline_flow_segments(inlines, base_style.clone(), style, &mut children);

    if children.is_empty() {
        children.push(div().child("\u{200B}").into_any_element());
    }

    div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_start()
        .gap_y(px(4.0))
        .children(children)
        .into_any_element()
}

fn append_inline_flow_segments(
    inlines: &[MarkdownInline],
    current_style: TextStyle,
    style: &ChatMarkdownStyle<'_>,
    children: &mut Vec<AnyElement>,
) {
    for inline in inlines {
        match inline {
            MarkdownInline::Text(value) => {
                append_text_flow_segments(value, &current_style, children);
            }
            MarkdownInline::Code(value) => {
                children.push(render_inline_code_chip(value, &current_style, style));
            }
            MarkdownInline::Emphasis(children_inlines) => {
                let mut emphasis = current_style.clone();
                emphasis.font_style = FontStyle::Italic;
                append_inline_flow_segments(children_inlines, emphasis, style, children);
            }
            MarkdownInline::Strong(children_inlines) => {
                let mut strong = current_style.clone();
                strong.font_weight = FontWeight::SEMIBOLD;
                append_inline_flow_segments(children_inlines, strong, style, children);
            }
            MarkdownInline::Strikethrough(children_inlines) => {
                let mut struck = current_style.clone();
                struck.strikethrough = Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(current_style.color.opacity(0.55)),
                    ..Default::default()
                });
                append_inline_flow_segments(children_inlines, struck, style, children);
            }
            MarkdownInline::Link { label, .. } => {
                let mut link_style = current_style.clone();
                link_style.color = style.link_color;
                link_style.underline = Some(UnderlineStyle {
                    color: Some(style.link_color.opacity(0.48)),
                    thickness: px(1.0),
                    wavy: false,
                });
                append_inline_flow_segments(label, link_style, style, children);
            }
            MarkdownInline::SoftBreak => {
                children.push(render_inline_text_segment(" ", &current_style));
            }
            MarkdownInline::LineBreak => {
                children.push(div().w_full().h(px(0.0)).into_any_element());
            }
        }
    }
}

fn append_text_flow_segments(value: &str, text_style: &TextStyle, children: &mut Vec<AnyElement>) {
    let mut current = String::new();
    let mut whitespace = false;

    for ch in value.chars() {
        if ch == '\n' {
            if !current.is_empty() {
                children.push(render_inline_text_segment(&current, text_style));
                current.clear();
            }
            whitespace = false;
            children.push(div().w_full().h(px(0.0)).into_any_element());
            continue;
        }

        if ch.is_whitespace() {
            if !current.is_empty() && !whitespace {
                children.push(render_inline_text_segment(&current, text_style));
                current.clear();
            }
            if !whitespace {
                current.push(' ');
                whitespace = true;
            }
        } else {
            if whitespace && !current.is_empty() {
                children.push(render_inline_text_segment(&current, text_style));
                current.clear();
            }
            whitespace = false;
            current.push(ch);
        }
    }

    if !current.is_empty() {
        children.push(render_inline_text_segment(&current, text_style));
    }
}

fn render_inline_text_segment(content: &str, text_style: &TextStyle) -> AnyElement {
    let font_size = text_style_font_size(text_style);
    let line_height = text_style_line_height(text_style, font_size);
    let mut segment = div()
        .whitespace_nowrap()
        .font_family(text_style.font_family.clone())
        .text_size(font_size)
        .line_height(line_height)
        .text_color(text_style.color)
        .child(content.to_string());

    if text_style.font_weight != FontWeight::NORMAL {
        segment = segment.font_weight(text_style.font_weight);
    }
    if text_style.font_style == FontStyle::Italic {
        segment = segment.italic();
    }
    if let Some(underline) = &text_style.underline {
        segment = segment
            .underline()
            .text_decoration_color(underline.color.unwrap_or(text_style.color));
        segment = if underline.wavy {
            segment.text_decoration_wavy()
        } else {
            segment.text_decoration_solid()
        };
    }
    if text_style.strikethrough.is_some() {
        segment = segment.line_through();
    }

    segment.into_any_element()
}

fn render_inline_code_chip(
    value: &str,
    text_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let font_size = text_style_font_size(text_style);
    let line_height = text_style_line_height(text_style, font_size);
    div()
        .whitespace_nowrap()
        .mx(px(1.0))
        .px(style.inline_code_padding_x)
        .py(style.inline_code_padding_y)
        .rounded(style.inline_code_radius)
        .bg(style.inline_code_background)
        .font_family(style.theme.mono_font_family.clone())
        .font_weight(FontWeight::MEDIUM)
        .text_size(font_size)
        .line_height(line_height)
        .text_color(style.inline_code_text_color)
        .child(value.replace(' ', "\u{00A0}"))
        .into_any_element()
}

fn text_style_font_size(text_style: &TextStyle) -> gpui::Pixels {
    match text_style.font_size {
        AbsoluteLength::Pixels(size) => size,
        _ => px(14.0),
    }
}

fn text_style_line_height(text_style: &TextStyle, font_size: gpui::Pixels) -> gpui::Pixels {
    match text_style.line_height {
        DefiniteLength::Absolute(AbsoluteLength::Pixels(size)) => size,
        _ => font_size * 1.5,
    }
}

fn inline_runs(
    inlines: &[MarkdownInline],
    base_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> (SharedString, Vec<gpui::TextRun>) {
    let mut text = String::new();
    let mut runs = Vec::new();
    append_inline_runs(inlines, base_style.clone(), style, &mut text, &mut runs);
    if text.is_empty() {
        text.push('\u{200B}');
        runs.push(base_style.to_run(text.len()));
    }
    (text.into(), runs)
}

fn append_inline_runs(
    inlines: &[MarkdownInline],
    current_style: TextStyle,
    style: &ChatMarkdownStyle<'_>,
    text: &mut String,
    runs: &mut Vec<gpui::TextRun>,
) {
    for inline in inlines {
        match inline {
            MarkdownInline::Text(value) => push_run(text, runs, &current_style, value),
            MarkdownInline::Code(value) => {
                let mut code_style = current_style.clone();
                code_style.font_family = style.theme.mono_font_family.clone();
                code_style.background_color = Some(style.inline_code_background);
                code_style.font_weight = FontWeight::MEDIUM;
                push_run(text, runs, &code_style, value);
            }
            MarkdownInline::Emphasis(children) => {
                let mut emphasis = current_style.clone();
                emphasis.font_style = FontStyle::Italic;
                append_inline_runs(children, emphasis, style, text, runs);
            }
            MarkdownInline::Strong(children) => {
                let mut strong = current_style.clone();
                strong.font_weight = FontWeight::SEMIBOLD;
                append_inline_runs(children, strong, style, text, runs);
            }
            MarkdownInline::Strikethrough(children) => {
                let mut struck = current_style.clone();
                struck.strikethrough = Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(current_style.color.opacity(0.55)),
                    ..Default::default()
                });
                append_inline_runs(children, struck, style, text, runs);
            }
            MarkdownInline::Link { label, .. } => {
                let mut link_style = current_style.clone();
                link_style.color = style.link_color;
                link_style.underline = Some(UnderlineStyle {
                    color: Some(style.link_color.opacity(0.48)),
                    thickness: px(1.0),
                    wavy: false,
                });
                append_inline_runs(label, link_style, style, text, runs);
            }
            MarkdownInline::SoftBreak => push_run(text, runs, &current_style, " "),
            MarkdownInline::LineBreak => push_run(text, runs, &current_style, "\n"),
        }
    }
}

fn push_run(text: &mut String, runs: &mut Vec<gpui::TextRun>, style: &TextStyle, content: &str) {
    if content.is_empty() {
        return;
    }

    text.push_str(content);
    runs.push(style.to_run(content.len()));
}

fn preserve_code_indentation(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut preserving_indent = true;

    for ch in line.chars() {
        if preserving_indent {
            match ch {
                ' ' => output.push('\u{00A0}'),
                '\t' => output.push_str("\u{00A0}\u{00A0}\u{00A0}\u{00A0}"),
                _ => {
                    preserving_indent = false;
                    output.push(ch);
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_inline_code_and_lists() {
        let blocks = parse_markdown("- one `code`\n- two");
        assert!(matches!(blocks.first(), Some(MarkdownBlock::List { .. })));
    }

    #[test]
    fn preserves_heading_and_quote_blocks() {
        let blocks = parse_markdown("# Title\n\n> quoted");
        assert!(matches!(
            blocks.first(),
            Some(MarkdownBlock::Heading { .. })
        ));
        assert!(matches!(blocks.get(1), Some(MarkdownBlock::BlockQuote(_))));
    }

    #[test]
    fn soft_breaks_do_not_become_hard_breaks() {
        let blocks =
            parse_markdown("Root filesystem\n`/`: `340G used / 559G avail` out of `937G` total");
        let MarkdownBlock::Paragraph(inlines) = &blocks[0] else {
            panic!("expected paragraph");
        };

        assert!(
            inlines
                .iter()
                .any(|inline| matches!(inline, MarkdownInline::SoftBreak))
        );
        assert!(
            !inlines
                .iter()
                .any(|inline| matches!(inline, MarkdownInline::LineBreak))
        );
    }
}
