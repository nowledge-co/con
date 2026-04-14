use gpui::{
    AbsoluteLength, AnyElement, DefiniteLength, FontStyle, FontWeight, Hsla, IntoElement,
    ParentElement, SharedString, Styled, StyledText, TextStyle, UnderlineStyle, WhiteSpace, div,
    px,
};
use gpui_component::clipboard::Clipboard;
use gpui_component::highlighter::SyntaxHighlighter;
use gpui_component::{Colorize, Theme};
use markdown::{ParseOptions, mdast};
use ropey::Rope;

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
    Table {
        aligns: Vec<MarkdownTableAlign>,
        rows: Vec<Vec<Vec<MarkdownInline>>>,
    },
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownTableAlign {
    Left,
    Center,
    Right,
    None,
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
    code_block_body_background: Hsla,
    code_block_language_background: Hsla,
    code_block_language_text_color: Hsla,
    quote_background: Hsla,
    quote_tint: Hsla,
    rule_color: Hsla,
    link_color: Hsla,
    table_border: Hsla,
    table_header_background: Hsla,
    table_cell_background: Hsla,
    block_gap: gpui::Pixels,
    inner_gap: gpui::Pixels,
}

impl<'a> ChatMarkdownStyle<'a> {
    fn new(theme: &'a Theme, tone: ChatMarkdownTone) -> Self {
        match tone {
            ChatMarkdownTone::Message => Self {
                theme,
                tone,
                content_width: px(720.0),
                base_font_size: px(15.0),
                base_line_height: px(24.0),
                code_font_size: theme.mono_font_size,
                code_line_height: px(21.0),
                text_color: theme.foreground.opacity(0.88),
                muted_text_color: theme.muted_foreground.opacity(0.74),
                inline_code_background: theme
                    .secondary_active
                    .mix_oklab(theme.background, 0.26)
                    .opacity(0.96),
                inline_code_text_color: theme.foreground.opacity(0.96),
                code_block_background: theme.secondary.mix_oklab(theme.background, 0.56),
                code_block_body_background: theme.background.mix_oklab(theme.secondary, 0.90),
                code_block_language_background: theme
                    .secondary_active
                    .mix_oklab(theme.background, 0.24)
                    .opacity(0.92),
                code_block_language_text_color: theme.foreground.opacity(0.74),
                quote_background: theme.secondary.opacity(0.68),
                quote_tint: theme.primary.opacity(0.34),
                rule_color: theme.muted_foreground.opacity(0.16),
                link_color: theme.primary,
                table_border: theme.muted_foreground.opacity(0.10),
                table_header_background: theme.muted.opacity(0.10),
                table_cell_background: theme.background.opacity(0.96),
                block_gap: px(13.0),
                inner_gap: px(9.0),
            },
            ChatMarkdownTone::Thinking => Self {
                theme,
                tone,
                content_width: px(640.0),
                base_font_size: px(12.75),
                base_line_height: px(20.0),
                code_font_size: theme.mono_font_size,
                code_line_height: px(19.0),
                text_color: theme.muted_foreground.opacity(0.66),
                muted_text_color: theme.muted_foreground.opacity(0.58),
                inline_code_background: theme
                    .secondary_active
                    .mix_oklab(theme.background, 0.20)
                    .opacity(0.90),
                inline_code_text_color: theme.foreground.opacity(0.84),
                code_block_background: theme.secondary.mix_oklab(theme.background, 0.48),
                code_block_body_background: theme.background.mix_oklab(theme.secondary, 0.84),
                code_block_language_background: theme
                    .secondary_active
                    .mix_oklab(theme.background, 0.18)
                    .opacity(0.82),
                code_block_language_text_color: theme.foreground.opacity(0.68),
                quote_background: theme.secondary.opacity(0.46),
                quote_tint: theme.primary.opacity(0.24),
                rule_color: theme.muted_foreground.opacity(0.12),
                link_color: theme.primary.opacity(0.82),
                table_border: theme.muted_foreground.opacity(0.08),
                table_header_background: theme.muted.opacity(0.08),
                table_cell_background: theme.background.opacity(0.82),
                block_gap: px(10.0),
                inner_gap: px(8.0),
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
    match markdown::to_mdast(source, &ParseOptions::gfm()) {
        Ok(mdast::Node::Root(root)) => root.children.iter().filter_map(parse_block_node).collect(),
        Ok(node) => parse_block_node(&node).into_iter().collect(),
        Err(_) => vec![MarkdownBlock::Paragraph(vec![MarkdownInline::Text(
            source.to_string(),
        )])],
    }
}

fn parse_block_node(node: &mdast::Node) -> Option<MarkdownBlock> {
    match node {
        mdast::Node::Paragraph(val) => {
            Some(MarkdownBlock::Paragraph(parse_inline_nodes(&val.children)))
        }
        mdast::Node::Heading(val) => Some(MarkdownBlock::Heading {
            level: val.depth,
            inlines: parse_inline_nodes(&val.children),
        }),
        mdast::Node::Code(raw) => Some(MarkdownBlock::CodeBlock {
            language: raw.lang.clone().filter(|lang| !lang.trim().is_empty()),
            code: raw.value.clone(),
        }),
        mdast::Node::Blockquote(val) => Some(MarkdownBlock::BlockQuote(
            val.children.iter().filter_map(parse_block_node).collect(),
        )),
        mdast::Node::List(list) => Some(MarkdownBlock::List {
            ordered: list.ordered,
            start: list.start.unwrap_or(1) as usize,
            items: list
                .children
                .iter()
                .filter_map(|item| match item {
                    mdast::Node::ListItem(list_item) => Some(
                        list_item
                            .children
                            .iter()
                            .filter_map(parse_block_node)
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .collect(),
        }),
        mdast::Node::ThematicBreak(_) => Some(MarkdownBlock::Rule),
        mdast::Node::Table(table) => {
            let rows = table
                .children
                .iter()
                .filter_map(|row| match row {
                    mdast::Node::TableRow(row) => Some(
                        row.children
                            .iter()
                            .filter_map(|cell| match cell {
                                mdast::Node::TableCell(cell) => {
                                    Some(parse_inline_nodes(&cell.children))
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some(MarkdownBlock::Table {
                aligns: table.align.iter().map(parse_table_align).collect(),
                rows,
            })
        }
        mdast::Node::Html(raw) => {
            let trimmed = raw.value.trim();
            (!trimmed.is_empty())
                .then(|| MarkdownBlock::Paragraph(vec![MarkdownInline::Text(trimmed.to_string())]))
        }
        mdast::Node::Yaml(val) => Some(MarkdownBlock::CodeBlock {
            language: Some("yml".to_string()),
            code: val.value.clone(),
        }),
        mdast::Node::Toml(val) => Some(MarkdownBlock::CodeBlock {
            language: Some("toml".to_string()),
            code: val.value.clone(),
        }),
        mdast::Node::Math(val) => Some(MarkdownBlock::CodeBlock {
            language: None,
            code: val.value.clone(),
        }),
        mdast::Node::FootnoteDefinition(def) => Some(MarkdownBlock::Paragraph(
            std::iter::once(MarkdownInline::Text(format!("[{}]: ", def.identifier)))
                .chain(parse_inline_nodes(&def.children))
                .collect(),
        )),
        _ => None,
    }
}

fn parse_table_align(align: &markdown::mdast::AlignKind) -> MarkdownTableAlign {
    match align {
        markdown::mdast::AlignKind::Left => MarkdownTableAlign::Left,
        markdown::mdast::AlignKind::Right => MarkdownTableAlign::Right,
        markdown::mdast::AlignKind::Center => MarkdownTableAlign::Center,
        markdown::mdast::AlignKind::None => MarkdownTableAlign::None,
    }
}

fn parse_inline_nodes(nodes: &[mdast::Node]) -> Vec<MarkdownInline> {
    let mut inlines = Vec::new();
    for node in nodes {
        match node {
            mdast::Node::Text(val) => push_text_fragments(&mut inlines, &val.value),
            mdast::Node::InlineCode(val) => inlines.push(MarkdownInline::Code(val.value.clone())),
            mdast::Node::InlineMath(val) => inlines.push(MarkdownInline::Code(val.value.clone())),
            mdast::Node::Emphasis(val) => {
                inlines.push(MarkdownInline::Emphasis(parse_inline_nodes(&val.children)))
            }
            mdast::Node::Strong(val) => {
                inlines.push(MarkdownInline::Strong(parse_inline_nodes(&val.children)))
            }
            mdast::Node::Delete(val) => inlines.push(MarkdownInline::Strikethrough(
                parse_inline_nodes(&val.children),
            )),
            mdast::Node::Link(val) => inlines.push(MarkdownInline::Link {
                label: parse_inline_nodes(&val.children),
                destination: val.url.clone(),
            }),
            mdast::Node::LinkReference(val) => inlines.push(MarkdownInline::Link {
                label: parse_inline_nodes(&val.children),
                destination: val.identifier.clone(),
            }),
            mdast::Node::Image(val) => {
                let label = if val.alt.is_empty() {
                    val.url.clone()
                } else {
                    val.alt.clone()
                };
                push_text_fragments(&mut inlines, &label);
            }
            mdast::Node::ImageReference(val) => {
                let label = if val.alt.is_empty() {
                    val.identifier.clone()
                } else {
                    val.alt.clone()
                };
                push_text_fragments(&mut inlines, &label);
            }
            mdast::Node::Break(_) => inlines.push(MarkdownInline::LineBreak),
            mdast::Node::FootnoteReference(val) => {
                push_text_fragments(&mut inlines, &format!("[{}]", val.identifier));
            }
            mdast::Node::Html(val) => {
                push_text_fragments(&mut inlines, &val.value);
            }
            mdast::Node::MdxTextExpression(val) => {
                push_text_fragments(&mut inlines, &val.value);
            }
            mdast::Node::MdxJsxTextElement(val) => {
                inlines.extend(parse_inline_nodes(&val.children));
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

fn push_text_fragments(inlines: &mut Vec<MarkdownInline>, text: &str) {
    if text.is_empty() {
        return;
    }

    let mut parts = text.split('\n').peekable();
    while let Some(part) = parts.next() {
        if !part.is_empty() {
            push_text(inlines, part);
        }
        if parts.peek().is_some() {
            inlines.push(MarkdownInline::SoftBreak);
        }
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
                let marker_lane_width = if *ordered {
                    ordered_list_marker_lane_width(start + items.len().saturating_sub(1))
                } else {
                    px(14.0)
                };

                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(9.0))
                    .child(
                        div()
                            .flex_none()
                            .pt(px(1.0))
                            .w(marker_lane_width)
                            .text_right()
                            .font_family(style.theme.mono_font_family.clone())
                            .text_size(style.base_font_size)
                            .line_height(style.base_line_height)
                            .text_color(style.muted_text_color)
                            .child(marker),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(7.0))
                            .flex_1()
                            .children(item_blocks.iter().enumerate().map(
                                |(nested_idx, nested_block)| {
                                    render_block(nested_block, nested_idx, style)
                                },
                            )),
                    )
                    .into_any_element()
            }))
            .into_any_element(),
        MarkdownBlock::Table { aligns, rows } => render_table_block(aligns, rows, style),
        MarkdownBlock::Rule => div()
            .w_full()
            .h(px(1.0))
            .bg(style.rule_color)
            .into_any_element(),
    }
}

fn ordered_list_marker_lane_width(max_marker: usize) -> gpui::Pixels {
    let digits = max_marker.max(1).to_string().len() as f32;
    px(14.0 + digits * 8.0)
}

fn render_table_block(
    aligns: &[MarkdownTableAlign],
    rows: &[Vec<Vec<MarkdownInline>>],
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    if rows.is_empty() {
        return div().into_any_element();
    }

    let header = &rows[0];
    let body_rows = rows.iter().skip(1).enumerate().map(|(row_idx, row)| {
        div()
            .w_full()
            .flex()
            .bg(if row_idx % 2 == 0 {
                style.table_cell_background
            } else {
                style.table_cell_background.opacity(0.96)
            })
            .children(row.iter().enumerate().map(|(column_idx, cell)| {
                render_table_cell(cell, column_idx, aligns, false, style)
            }))
            .into_any_element()
    });

    div()
        .w_full()
        .overflow_hidden()
        .rounded(px(11.0))
        .bg(style.table_border)
        .p(px(1.0))
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .bg(style.table_border)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .bg(style.table_header_background)
                        .children(header.iter().enumerate().map(|(column_idx, cell)| {
                            render_table_cell(cell, column_idx, aligns, true, style)
                        })),
                )
                .children(body_rows),
        )
        .into_any_element()
}

fn render_table_cell(
    cell: &[MarkdownInline],
    column_idx: usize,
    aligns: &[MarkdownTableAlign],
    is_header: bool,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let base_style = if is_header {
        let mut header_style = style.base_text_style();
        header_style.font_weight = FontWeight::SEMIBOLD;
        header_style.color = style.text_color.opacity(0.96);
        header_style
    } else {
        style.base_text_style()
    };

    let align = aligns
        .get(column_idx)
        .copied()
        .unwrap_or(MarkdownTableAlign::Left);

    let content = render_inline_content(cell, &base_style, style);

    let cell = div()
        .flex_1()
        .min_w(px(84.0))
        .min_w_0()
        .px(px(11.0))
        .py(px(if is_header { 8.0 } else { 9.0 }))
        .child(match align {
            MarkdownTableAlign::Center => div().w_full().text_center().child(content),
            MarkdownTableAlign::Right => div().w_full().text_right().child(content),
            MarkdownTableAlign::Left | MarkdownTableAlign::None => div().w_full().child(content),
        });

    if column_idx > 0 {
        div()
            .flex()
            .bg(style.table_border)
            .child(div().w(px(1.0)).self_stretch())
            .child(cell)
            .into_any_element()
    } else {
        cell.into_any_element()
    }
}

fn render_code_block(
    _index: usize,
    language: &Option<String>,
    code: &str,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let mono_style = style.code_text_style();
    let lines: Vec<&str> = code.lines().collect();
    let has_trailing_newline = code.ends_with('\n');
    let header_label = language
        .as_deref()
        .filter(|lang| !lang.trim().is_empty())
        .unwrap_or("code");

    let header_row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(8.0))
                .bg(style.code_block_language_background)
                .font_family(style.theme.mono_font_family.clone())
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(10.5))
                .line_height(px(11.0))
                .text_color(style.code_block_language_text_color)
                .child(header_label.to_string()),
        )
        .child(div().h(px(1.0)).flex_1().bg(style.rule_color.opacity(0.36)))
        .child(
            Clipboard::new(format!("copy-code-block-{_index}"))
                .value(SharedString::from(code.to_string())),
        );

    let block = div()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(13.0))
        .bg(style.code_block_background.opacity(0.98))
        .p(px(1.0))
        .child(
            div()
                .overflow_hidden()
                .rounded(px(12.0))
                .bg(style
                    .code_block_background
                    .mix_oklab(style.code_block_body_background, 0.82))
                .child(
                    div()
                        .px(px(14.0))
                        .pt(px(10.0))
                        .pb(px(8.0))
                        .child(header_row),
                ),
        );

    let mut code_column = div().flex().flex_col().gap(px(0.0)).w_full();
    let syntax_runs = highlighted_code_runs(code, language, style);

    for (line_idx, line) in lines.iter().enumerate() {
        let display_line = if line.is_empty() { "\u{200B}" } else { line };
        code_column = code_column.child(
            div()
                .w_full()
                .min_h(style.code_line_height)
                .font_family(style.theme.mono_font_family.clone())
                .text_size(style.code_font_size)
                .line_height(style.code_line_height)
                .text_color(style.text_color.opacity(0.96))
                .child(
                    match syntax_runs
                        .as_ref()
                        .and_then(|runs| runs.get(line_idx).cloned())
                    {
                        Some(runs) => StyledText::new(display_line.to_string()).with_runs(runs),
                        None => StyledText::new(display_line.to_string())
                            .with_runs(vec![mono_style.to_run(display_line.len())]),
                    },
                ),
        );
    }

    if lines.is_empty() || has_trailing_newline {
        code_column = code_column.child(
            div()
                .h(style.code_line_height)
                .font_family(style.theme.mono_font_family.clone()),
        );
    }

    block
        .child(
            div().px(px(10.0)).pb(px(10.0)).child(
                div()
                    .rounded(px(10.0))
                    .bg(style.code_block_body_background.opacity(0.985))
                    .px(px(12.0))
                    .py(px(11.0))
                    .child(code_column),
            ),
        )
        .into_any_element()
}

fn highlighted_code_runs(
    code: &str,
    language: &Option<String>,
    style: &ChatMarkdownStyle<'_>,
) -> Option<Vec<Vec<gpui::TextRun>>> {
    let lang = canonical_highlighter_language(language.as_deref()?);
    if lang.is_empty() || suppress_syntax_highlighting(lang) {
        return None;
    }

    let rope = Rope::from_str(code);
    let mut highlighter = SyntaxHighlighter::new(lang);
    highlighter.update(None, &rope, None);
    let highlights = highlighter.styles(&(0..code.len()), &style.theme.highlight_theme);

    let base_style = style.code_text_style();
    let mut line_runs = Vec::new();
    let mut line_start = 0usize;

    for line in code.lines() {
        let line_end = line_start + line.len();
        let display_len = if line.is_empty() {
            "\u{200B}".len()
        } else {
            line.len()
        };
        let mut runs = Vec::new();
        let mut cursor = line_start;

        for (range, highlight) in &highlights {
            let start = range.start.max(line_start);
            let end = range.end.min(line_end);
            if start >= end {
                continue;
            }

            if start > cursor {
                runs.push(base_style.to_run(start - cursor));
            }

            let mut highlighted_style = base_style.clone();
            apply_code_highlight_style(&mut highlighted_style, *highlight, style);
            runs.push(highlighted_style.to_run(end - start));
            cursor = end;
        }

        if cursor < line_end {
            runs.push(base_style.to_run(line_end - cursor));
        }

        if runs.is_empty() {
            runs.push(base_style.to_run(display_len));
        }

        line_runs.push(runs);
        line_start = line_end + 1;
    }

    Some(line_runs)
}

fn canonical_highlighter_language(language: &str) -> &str {
    match language.trim().to_ascii_lowercase().as_str() {
        "sh" | "shell" | "zsh" | "console" | "terminal" => "bash",
        other => {
            if other.is_empty() {
                ""
            } else {
                language.trim()
            }
        }
    }
}

fn suppress_syntax_highlighting(lang: &str) -> bool {
    matches!(lang.to_ascii_lowercase().as_str(), "text" | "txt" | "plain")
}

fn apply_code_highlight_style(
    text_style: &mut TextStyle,
    highlight: gpui::HighlightStyle,
    style: &ChatMarkdownStyle<'_>,
) {
    let base_color = if style.theme.is_dark() {
        style.text_color.opacity(0.96)
    } else {
        style.text_color.opacity(0.90)
    };
    text_style.font_family = style.theme.mono_font_family.clone();
    text_style.font_size = style.code_font_size.into();
    text_style.line_height = style.code_line_height.into();
    text_style.font_style = FontStyle::Normal;
    text_style.font_weight = FontWeight::NORMAL;
    text_style.background_color = None;
    text_style.underline = None;
    text_style.strikethrough = None;
    text_style.color = highlight
        .color
        .map(|color| color.mix_oklab(base_color, 0.76).opacity(0.99))
        .unwrap_or(base_color);
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
        .items_baseline()
        .gap_x(px(0.0))
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
                append_text_flow_segments(value, &current_style, children)
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
            MarkdownInline::SoftBreak => append_text_flow_segments(" ", &current_style, children),
            MarkdownInline::LineBreak => {
                children.push(div().w_full().h(px(0.0)).into_any_element());
            }
        }
    }
}

fn append_text_flow_segments(value: &str, text_style: &TextStyle, children: &mut Vec<AnyElement>) {
    for segment in tokenize_inline_text(value) {
        children.push(render_inline_text_segment(&segment, text_style));
    }
}

fn tokenize_inline_text(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut trailing_whitespace = String::new();

    for part in value.split_inclusive(char::is_whitespace) {
        if part.chars().all(char::is_whitespace) {
            trailing_whitespace.push_str(part);
        } else if part.chars().last().is_some_and(char::is_whitespace) {
            tokens.push(part.replace(' ', "\u{00A0}"));
        } else if trailing_whitespace.is_empty() {
            tokens.push(part.to_string());
        } else {
            tokens.push(format!(
                "{}{}",
                trailing_whitespace.replace(' ', "\u{00A0}"),
                part
            ));
            trailing_whitespace.clear();
        }
    }

    if !trailing_whitespace.is_empty() {
        tokens.push(trailing_whitespace.replace(' ', "\u{00A0}"));
    }

    tokens
}

fn render_inline_text_segment(content: &str, text_style: &TextStyle) -> AnyElement {
    let font_size = text_style_font_size(text_style);
    let line_height = text_style_line_height(text_style, font_size);
    let mut segment = div()
        .whitespace_nowrap()
        .flex_none()
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
    _text_style: &TextStyle,
    style: &ChatMarkdownStyle<'_>,
) -> AnyElement {
    let font_size = style.code_font_size - px(0.5);
    let line_height = style.code_line_height - px(3.0);
    div()
        .flex()
        .flex_none()
        .items_center()
        .mx(px(1.0))
        .my(px(0.5))
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(6.0))
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
                code_style.color = style.inline_code_text_color;
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

    #[test]
    fn parses_tables_from_mdast() {
        let blocks = parse_markdown("| Name | Value |\n| --- | ---: |\n| foo | `bar` |");
        let MarkdownBlock::Table { aligns, rows } = &blocks[0] else {
            panic!("expected table");
        };

        assert_eq!(aligns.len(), 2);
        assert_eq!(rows.len(), 2);
        assert!(matches!(aligns[1], MarkdownTableAlign::Right));
    }

    #[test]
    fn highlighted_code_runs_cover_empty_lines() {
        let theme = Theme::default();
        let style = ChatMarkdownStyle::new(&theme, ChatMarkdownTone::Message);
        let runs = highlighted_code_runs("print(1)\n\nprint(2)", &Some("python".into()), &style)
            .expect("expected syntax runs");

        assert_eq!(runs.len(), 3);
        assert_eq!(
            runs[1].iter().map(|run| run.len).sum::<usize>(),
            "\u{200B}".len()
        );
    }
}
