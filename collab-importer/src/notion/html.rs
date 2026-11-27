//! Converts a page of a Notion HTML export into a document.
//!
//! The HTML export keeps what the Markdown export drops: page icons and covers, columns, block
//! and text colors, callout icons and the widths of tables. The links and image paths of a page
//! are left as they are in the export, relative and percent-encoded, so that they are resolved
//! like the ones of an imported Markdown page.

use collab_document::blocks::{Block, BlockType, DocumentData, DocumentMeta};
use collab_document::document_data::generate_id;
use collab_document::importer::define::*;
use collab_document::importer::delta::Delta;
use collab_document::importer::util::BlockData;
use scraper::node::Node;
use scraper::{ElementRef, Html, Selector};
use serde_json::{Value, json};
use std::collections::HashMap;

const BLOCK_BG_COLOR_FIELD: &str = "bgColor";
const FONT_COLOR_ATTR: &str = "font_color";
const BG_COLOR_ATTR: &str = "bg_color";
const UNDERLINE_ATTR: &str = "underline";
const CALLOUT_ICON_FIELD: &str = "icon";
const COLLAPSED_FIELD: &str = "collapsed";
const COLUMN_RATIO_FIELD: &str = "ratio";
const ENABLE_HEADER_ROW_FIELD: &str = "enable_header_row";
const ENABLE_HEADER_COLUMN_FIELD: &str = "enable_header_column";
const COLUMN_WIDTHS_FIELD: &str = "column_widths";

/// The data of a paragraph that stands for a database that the page shows. Its value is the title
/// of the database, and the importer replaces the paragraph with a link to the database.
pub(crate) const INLINE_DATABASE_FIELD: &str = "notion_database";

/// The icon of a Notion page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageIcon {
  Emoji(String),
  /// An image, either a link to a Notion icon or the path of a file of the export.
  Image(String),
}

/// A page of a Notion HTML export.
#[derive(Debug)]
pub struct NotionHtmlPage {
  pub icon: Option<PageIcon>,
  /// The cover image, either a link or the path of a file of the export.
  pub cover: Option<String>,
  /// How far the cover is moved, from -1 (top of the image shown) to 1 (bottom of it).
  pub cover_offset: Option<f64>,
  pub document: DocumentData,
  /// The `href` of every link of the page, in order.
  pub links: Vec<String>,
  /// Whether the page has content besides its properties.
  pub has_content: bool,
}

/// Parse a page of a Notion HTML export.
pub fn parse_notion_html(document_id: &str, html: &str) -> NotionHtmlPage {
  let html = Html::parse_document(html);
  let mut converter = Converter::new(document_id);

  if let Some(body) = select_first(&html, ".page-body") {
    converter.children_as_blocks(document_id, body);
  }

  let has_content = converter
    .data
    .meta
    .children_map
    .get(document_id)
    .is_some_and(|children| !children.is_empty());
  let header = page_header(&html);
  NotionHtmlPage {
    icon: header.icon,
    cover: header.cover,
    cover_offset: header.cover_offset,
    document: converter.data,
    links: converter.links,
    has_content,
  }
}

fn select_first<'a>(html: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
  let selector = Selector::parse(selector).ok()?;
  html.select(&selector).next()
}

/// The icon and the cover of a page of a Notion HTML export.
pub struct PageHeader {
  pub icon: Option<PageIcon>,
  pub cover: Option<String>,
  pub cover_offset: Option<f64>,
}

/// Parse the icon and the cover of a page of a Notion HTML export.
pub fn parse_notion_html_header(html: &str) -> PageHeader {
  page_header(&Html::parse_document(html))
}

/// The names of the columns of the database that a page of a Notion HTML export shows, in the
/// order of its table. Columns that the view hides are not part of it.
pub fn parse_notion_html_columns(html: &str) -> Vec<String> {
  let html = Html::parse_document(html);
  let Ok(selector) = Selector::parse("table.collection-content thead th") else {
    return vec![];
  };
  html
    .select(&selector)
    .map(|th| th.text().collect::<String>().trim().to_string())
    .collect()
}

fn page_header(html: &Html) -> PageHeader {
  let icon = select_first(html, "article")
    .and_then(|article| article.value().attr("data-notion-page-icon"))
    .filter(|icon| !icon.is_empty())
    .and_then(page_icon)
    .or_else(|| {
      select_first(html, ".page-header-icon img")
        .and_then(|img| img.value().attr("src"))
        .and_then(page_icon)
    });
  let cover_image = select_first(html, "img.page-cover-image");
  let cover = cover_image
    .and_then(|img| img.value().attr("src"))
    .map(str::to_string);
  // The style of the image has the part of it that is shown, such as `object-position:center 21%`.
  let cover_offset = cover_image
    .and_then(|img| img.value().attr("style"))
    .and_then(|style| style.split("object-position:").nth(1))
    .and_then(|position| position.split(';').next())
    .and_then(|position| position.split_whitespace().nth(1))
    .and_then(|y| y.trim_end_matches('%').parse::<f64>().ok())
    .map(|percent| (percent / 50.0 - 1.0).clamp(-1.0, 1.0));
  PageHeader {
    icon,
    cover,
    cover_offset,
  }
}

/// The icon of a page from the emoji, the path of a Notion icon or the path of an image that the
/// export has for it.
fn page_icon(icon: &str) -> Option<PageIcon> {
  if !icon.contains('/') {
    return Some(PageIcon::Emoji(icon.to_string()));
  }
  match notion_icon_name(icon) {
    // Notion's own icons are not part of AppFlowy, an emoji that looks like them is used instead.
    Some(name) => notion_icon_emoji(name).map(|emoji| PageIcon::Emoji(emoji.to_string())),
    None => Some(PageIcon::Image(icon.to_string())),
  }
}

/// The name of the icon of Notion's set that a path points to, such as `bug` for
/// `https://app.notion.com/icons/bug_gray.svg`.
fn notion_icon_name(path: &str) -> Option<&str> {
  let file = path.rsplit_once("/icons/")?.1.strip_suffix(".svg")?;
  Some(file.rsplit_once('_').map_or(file, |(name, _color)| name))
}

/// An emoji that stands for an icon of Notion's set.
fn notion_icon_emoji(name: &str) -> Option<&'static str> {
  Some(match name {
    "activity" => "📈",
    "archery" => "🎯",
    "bell" | "new-alert" => "🔔",
    "book" => "📖",
    "bookmark" => "🔖",
    "bug" => "🐞",
    "calendar" => "📅",
    "camera" => "📷",
    "chart-area" | "chart-bar" | "chart-line" => "📊",
    "checklist" => "✅",
    "clock" => "🕐",
    "cloud" => "☁️",
    "code" => "💻",
    "compose" | "edit" => "✏️",
    "database" => "🗄️",
    "departures" => "🛫",
    "dialogue" | "comment" => "💬",
    "document" | "page" => "📄",
    "drafts" => "📝",
    "fire" => "🔥",
    "flag" => "🚩",
    "folder" | "new-folder" => "📁",
    "friends" | "users" => "👥",
    "gift" => "🎁",
    "globe" | "world" => "🌐",
    "heart" => "❤️",
    "heart-rate" => "💓",
    "help-alternate" | "help" => "❓",
    "home" => "🏠",
    "image" | "picture" => "🖼️",
    "info" => "ℹ️",
    "key" => "🔑",
    "lightbulb" | "idea" => "💡",
    "link" => "🔗",
    "location" => "📍",
    "lock" => "🔒",
    "lungs" => "🫁",
    "mail" | "email" => "✉️",
    "music" => "🎵",
    "news" => "📰",
    "notebook" => "📓",
    "phone" => "📱",
    "phone-speaker" => "📞",
    "playback-play-button" | "play" => "▶️",
    "rocket" => "🚀",
    "search" => "🔍",
    "send-to" => "📤",
    "settings" | "gear" => "⚙️",
    "shield" => "🛡️",
    "star" => "⭐",
    "swap-horizontally" => "🔄",
    "tag" => "🏷️",
    "theatre" => "🎭",
    "ticket" => "🎫",
    "tools" | "wrench" => "🔧",
    "trash" => "🗑️",
    "trophy" => "🏆",
    "user" | "person" => "👤",
    "video" => "🎥",
    "view" => "👁️",
    "warning" => "⚠️",
    "wifi" => "📶",
    _ => return None,
  })
}

struct Converter {
  data: DocumentData,
  links: Vec<String>,
}

type Attributes = Vec<(String, Value)>;

impl Converter {
  fn new(document_id: &str) -> Self {
    let mut data = DocumentData {
      page_id: document_id.to_string(),
      blocks: HashMap::new(),
      meta: DocumentMeta {
        children_map: HashMap::new(),
        text_map: Some(HashMap::new()),
      },
    };
    data.blocks.insert(
      document_id.to_string(),
      Block {
        id: document_id.to_string(),
        ty: BlockType::Page.to_string(),
        data: BlockData::new(),
        parent: String::new(),
        children: document_id.to_string(),
        external_id: Some(document_id.to_string()),
        external_type: Some("text".to_string()),
      },
    );
    data
      .meta
      .children_map
      .insert(document_id.to_string(), vec![]);
    Self {
      data,
      links: vec![],
    }
  }

  /// Add a block under `parent` and return its id.
  fn add_block(&mut self, ty: BlockType, parent: &str, data: BlockData) -> String {
    let id = generate_id();
    let has_text = matches!(
      ty,
      BlockType::Paragraph
        | BlockType::Heading
        | BlockType::BulletedList
        | BlockType::NumberedList
        | BlockType::TodoList
        | BlockType::ToggleList
        | BlockType::Quote
        | BlockType::Callout
        | BlockType::Code
    );
    let has_children = !matches!(
      ty,
      BlockType::Image | BlockType::Divider | BlockType::LinkPreview | BlockType::Code
    );
    self.data.blocks.insert(
      id.clone(),
      Block {
        id: id.clone(),
        ty: ty.to_string(),
        data,
        parent: parent.to_string(),
        children: if has_children {
          id.clone()
        } else {
          String::new()
        },
        external_id: has_text.then(|| id.clone()),
        external_type: has_text.then(|| "text".to_string()),
      },
    );
    self
      .data
      .meta
      .children_map
      .entry(parent.to_string())
      .or_default()
      .push(id.clone());
    id
  }

  fn append_delta(&mut self, block_id: &str, delta: Delta) {
    if delta.ops.is_empty() {
      return;
    }
    let text_map = self.data.meta.text_map.get_or_insert_with(HashMap::new);
    let mut ops: Vec<Value> = text_map
      .get(block_id)
      .and_then(|text| serde_json::from_str(text).ok())
      .unwrap_or_default();
    if let Ok(Value::Array(added)) = serde_json::from_str::<Value>(&delta.to_json()) {
      ops.extend(added);
    }
    text_map.insert(
      block_id.to_string(),
      serde_json::to_string(&ops).unwrap_or_else(|_| "[]".to_string()),
    );
  }

  // ---------------------------------------------------------------------------------------------
  // Blocks
  // ---------------------------------------------------------------------------------------------

  /// Convert the children of `element` into blocks under `parent`. Text and inline elements
  /// that sit between blocks become paragraphs.
  fn children_as_blocks(&mut self, parent: &str, element: ElementRef) {
    let mut inline_run: Vec<ego_tree::NodeRef<Node>> = vec![];
    for child in element.children() {
      match child.value() {
        Node::Element(_) => {
          let child_element = ElementRef::wrap(child).expect("element node");
          if is_inline_element(child_element) {
            inline_run.push(child);
          } else {
            self.flush_inline_run(parent, &mut inline_run);
            self.element_as_block(parent, child_element);
          }
        },
        Node::Text(_) => inline_run.push(child),
        _ => {},
      }
    }
    self.flush_inline_run(parent, &mut inline_run);
  }

  fn flush_inline_run(&mut self, parent: &str, run: &mut Vec<ego_tree::NodeRef<Node>>) {
    if run.is_empty() {
      return;
    }
    let mut delta = Delta::new();
    for node in run.drain(..) {
      self.inline_node(node, &[], &mut delta);
    }
    if delta
      .ops
      .iter()
      .all(|op| op.insert().chars().all(char::is_whitespace))
    {
      return;
    }
    let id = self.add_block(BlockType::Paragraph, parent, BlockData::new());
    self.append_delta(&id, delta);
  }

  fn element_as_block(&mut self, parent: &str, element: ElementRef) {
    let name = element.value().name();
    match name {
      "p" => {
        let (data, attrs) = block_colors(element);
        let id = self.add_block(BlockType::Paragraph, parent, data);
        self.fill_text_block(&id, element, &attrs);
      },
      "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
        let level: u64 = name[1..].parse().unwrap_or(1);
        let (mut data, attrs) = block_colors(element);
        data.insert(LEVEL_FIELD.to_string(), json!(level.clamp(1, 6)));
        let id = self.add_block(BlockType::Heading, parent, data);
        self.fill_text_block(&id, element, &attrs);
      },
      "ul" | "ol" => self.list(parent, element),
      "details" => self.toggle(parent, element),
      "blockquote" => {
        let (data, attrs) = block_colors(element);
        let id = self.add_block(BlockType::Quote, parent, data);
        self.fill_text_block(&id, element, &attrs);
      },
      "aside" => self.callout(parent, element),
      "pre" => self.code(parent, element),
      "hr" => {
        self.add_block(BlockType::Divider, parent, BlockData::new());
      },
      "table" => {
        if has_class(element, "simple-table") {
          self.simple_table(parent, element);
        }
      },
      "figure" => self.figure(parent, element),
      "div" => {
        if has_class(element, "column-list") {
          self.columns(parent, element);
        } else if has_class(element, "collection-content") {
          self.inline_database(parent, element);
        } else {
          self.children_as_blocks(parent, element);
        }
      },
      // Containers of blocks, such as the body of a column or of a toggle.
      "section" | "article" | "main" => self.children_as_blocks(parent, element),
      _ => {},
    }
  }

  /// A database shown in the page is imported from its own CSV file. The page gets a paragraph
  /// with the title of the database, which the importer turns into a link to it.
  fn inline_database(&mut self, parent: &str, element: ElementRef) {
    let title = Selector::parse(".collection-title")
      .ok()
      .and_then(|selector| element.select(&selector).next())
      .map(|title| title.text().collect::<String>().trim().to_string())
      .unwrap_or_default();
    if title.is_empty() {
      return;
    }
    let mut data = BlockData::new();
    data.insert(INLINE_DATABASE_FIELD.to_string(), json!(title));
    let id = self.add_block(BlockType::Paragraph, parent, data);
    let mut delta = Delta::new();
    delta.insert(title, vec![]);
    self.append_delta(&id, delta);
  }

  /// Fill `block_id` with the text of `element` and the blocks nested in it.
  fn fill_text_block(&mut self, block_id: &str, element: ElementRef, attrs: &Attributes) {
    let mut delta = Delta::new();
    for child in element.children() {
      match child.value() {
        Node::Element(_) => {
          let child_element = ElementRef::wrap(child).expect("element node");
          if is_nested_block(child_element) {
            self.element_as_block(block_id, child_element);
          } else if has_class(child_element, "indented") {
            self.children_as_blocks(block_id, child_element);
          } else {
            self.inline_node(child, attrs, &mut delta);
          }
        },
        _ => self.inline_node(child, attrs, &mut delta),
      }
    }
    self.append_delta(block_id, trim_line_breaks(delta));
  }

  fn list(&mut self, parent: &str, list: ElementRef) {
    let class = list.value().attr("class").unwrap_or_default();
    let ordered = list.value().name() == "ol";
    let todo = class.contains("to-do-list");
    let mut number: u64 = list
      .value()
      .attr("start")
      .and_then(|start| start.parse().ok())
      .unwrap_or(1);
    let (list_data, attrs) = block_colors(list);
    for item in list.children().filter_map(ElementRef::wrap) {
      if item.value().name() != "li" {
        continue;
      }
      let mut data = list_data.clone();
      let ty = if todo {
        let checked = item
          .children()
          .filter_map(ElementRef::wrap)
          .any(|child| has_class(child, "checkbox-on"));
        data.insert(CHECKED_FIELD.to_string(), json!(checked));
        BlockType::TodoList
      } else if ordered {
        data.insert(START_NUMBER_FIELD.to_string(), json!(number));
        number += 1;
        BlockType::NumberedList
      } else {
        BlockType::BulletedList
      };
      let id = self.add_block(ty, parent, data);
      self.fill_text_block(&id, item, &attrs);
      if todo {
        self.trim_start_of_text(&id);
      }
    }
  }

  /// The text of a to-do item follows its checkbox, after a space.
  fn trim_start_of_text(&mut self, block_id: &str) {
    let Some(text_map) = self.data.meta.text_map.as_mut() else {
      return;
    };
    let Some(Value::Array(mut ops)) = text_map
      .get(block_id)
      .and_then(|text| serde_json::from_str(text).ok())
    else {
      return;
    };
    while let Some(Value::String(text)) = ops.first_mut().and_then(|op| op.get_mut("insert")) {
      *text = text.trim_start().to_string();
      if !text.is_empty() {
        break;
      }
      ops.remove(0);
    }
    text_map.insert(
      block_id.to_string(),
      serde_json::to_string(&ops).unwrap_or_default(),
    );
  }

  fn toggle(&mut self, parent: &str, details: ElementRef) {
    let (mut data, attrs) = block_colors(details);
    // The export marks every toggle as open, whatever its state in Notion, where a toggle starts
    // closed.
    data.insert(COLLAPSED_FIELD.to_string(), json!(true));
    let id = self.add_block(BlockType::ToggleList, parent, data);
    for child in details.children().filter_map(ElementRef::wrap) {
      match child.value().name() {
        "summary" => {
          let mut delta = Delta::new();
          for node in child.children() {
            self.inline_node(node, &attrs, &mut delta);
          }
          self.append_delta(&id, trim_line_breaks(delta));
        },
        _ if has_class(child, "indented") => self.children_as_blocks(&id, child),
        _ => self.element_as_block(&id, child),
      }
    }
  }

  fn callout(&mut self, parent: &str, aside: ElementRef) {
    let mut data = BlockData::new();
    if let Some(icon) = aside.value().attr("data-notion-callout-icon") {
      data.insert(CALLOUT_ICON_FIELD.to_string(), json!(icon));
    }
    if let Some(color) = aside
      .value()
      .attr("data-notion-callout-background")
      .and_then(|color| notion_color(color, true))
    {
      data.insert(BLOCK_BG_COLOR_FIELD.to_string(), json!(color));
    }
    let id = self.add_block(BlockType::Callout, parent, data);

    // The icon is the first child of the callout and its content the second one.
    let content = aside
      .children()
      .filter_map(ElementRef::wrap)
      .find(|child| child.value().name() == "div" && !has_inline_style(child, "font-size"))
      .unwrap_or(aside);

    // The first paragraph is the text of the callout, the rest are blocks in it.
    let mut first_paragraph_taken = false;
    for child in content.children() {
      let Some(child_element) = ElementRef::wrap(child) else {
        continue;
      };
      if !first_paragraph_taken && child_element.value().name() == "p" {
        first_paragraph_taken = true;
        self.fill_text_block(&id, child_element, &vec![]);
      } else {
        first_paragraph_taken = true;
        self.element_as_block(&id, child_element);
      }
    }
  }

  fn code(&mut self, parent: &str, pre: ElementRef) {
    let language = pre
      .value()
      .attr("data-notion-code-syntax")
      .map(str::to_string)
      .or_else(|| {
        pre
          .children()
          .filter_map(ElementRef::wrap)
          .find_map(|code| {
            code
              .value()
              .attr("class")?
              .split_whitespace()
              .find_map(|class| class.strip_prefix("language-"))
              .map(str::to_string)
          })
      })
      .unwrap_or_default();
    let mut data = BlockData::new();
    data.insert(LANGUAGE_FIELD.to_string(), json!(language));
    let id = self.add_block(BlockType::Code, parent, data);
    let text: String = pre.text().collect();
    let mut delta = Delta::new();
    delta.insert(text, vec![]);
    self.append_delta(&id, delta);
  }

  fn figure(&mut self, parent: &str, figure: ElementRef) {
    let class = figure.value().attr("class").unwrap_or_default();
    if class.contains("link-to-page") {
      // A paragraph with a link, which is turned into a mention when it points to an imported page.
      let id = self.add_block(BlockType::Paragraph, parent, BlockData::new());
      let mut delta = Delta::new();
      if let Some(link) = figure
        .children()
        .filter_map(ElementRef::wrap)
        .find(|link| link.value().name() == "a")
      {
        let title: String = link.text().collect();
        let mut attrs = vec![];
        if let Some(href) = link.value().attr("href") {
          self.links.push(href.to_string());
          attrs.push((HREF_ATTR.to_string(), json!(href)));
        }
        delta.insert(title.trim().to_string(), attrs);
      }
      self.append_delta(&id, delta);
      return;
    }

    if class.contains("image") {
      if let Some(src) = descendant_attr(figure, "img", "src") {
        let block_id = generate_id();
        let mut data = BlockData::new();
        data.insert(URL_FIELD.to_string(), json!(src));
        data.insert(IMAGE_TYPE_FIELD.to_string(), json!(EXTERNAL_IMAGE_TYPE));
        self.data.blocks.insert(
          block_id.clone(),
          Block {
            id: block_id.clone(),
            ty: BlockType::Image.to_string(),
            data,
            parent: parent.to_string(),
            children: String::new(),
            external_id: None,
            external_type: None,
          },
        );
        self
          .data
          .meta
          .children_map
          .entry(parent.to_string())
          .or_default()
          .push(block_id);
      }
      for caption in figure
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|child| child.value().name() == "figcaption")
      {
        let id = self.add_block(BlockType::Paragraph, parent, BlockData::new());
        self.fill_text_block(&id, caption, &vec![]);
      }
      return;
    }

    // A bookmark is a preview of the page it links to.
    if let Some(href) = figure
      .descendants()
      .filter_map(ElementRef::wrap)
      .find(|link| link.value().name() == "a" && has_class(*link, "bookmark"))
      .and_then(|link| link.value().attr("href"))
    {
      let mut data = BlockData::new();
      data.insert(URL_FIELD.to_string(), json!(href));
      self.add_block(BlockType::LinkPreview, parent, data);
      return;
    }

    // Other figures, such as files, are kept as a link.
    let mut delta = Delta::new();
    for link in figure
      .descendants()
      .filter_map(ElementRef::wrap)
      .filter(|link| link.value().name() == "a")
    {
      self.inline_node(*link, &[], &mut delta);
    }
    if !delta.ops.is_empty() {
      let id = self.add_block(BlockType::Paragraph, parent, BlockData::new());
      self.append_delta(&id, delta);
    }
  }

  fn columns(&mut self, parent: &str, list: ElementRef) {
    let columns: Vec<_> = list
      .children()
      .filter_map(ElementRef::wrap)
      .filter(|column| has_class(*column, "column"))
      .collect();
    if columns.is_empty() {
      return;
    }
    let columns_id = self.add_block(BlockType::SimpleColumns, parent, BlockData::new());
    for column in &columns {
      let ratio = column
        .value()
        .attr("data-notion-column-ratio")
        .and_then(|ratio| ratio.parse::<f64>().ok())
        .unwrap_or(1.0 / columns.len() as f64);
      let mut data = BlockData::new();
      data.insert(COLUMN_RATIO_FIELD.to_string(), json!(ratio));
      let column_id = self.add_block(BlockType::SimpleColumn, &columns_id, data);
      self.children_as_blocks(&column_id, *column);
    }
  }

  fn simple_table(&mut self, parent: &str, table: ElementRef) {
    let rows: Vec<_> = table
      .descendants()
      .filter_map(ElementRef::wrap)
      .filter(|row| row.value().name() == "tr")
      .collect();

    let mut data = BlockData::new();
    let header_row = rows.first().is_some_and(|row| {
      cells_of(*row)
        .iter()
        .all(|cell| cell.value().name() == "th")
    });
    let header_column = rows.iter().all(|row| {
      cells_of(*row)
        .first()
        .is_some_and(|cell| cell.value().name() == "th")
    });
    if header_row {
      data.insert(ENABLE_HEADER_ROW_FIELD.to_string(), json!(true));
    }
    if header_column {
      data.insert(ENABLE_HEADER_COLUMN_FIELD.to_string(), json!(true));
    }
    if let Some(first_row) = rows.first() {
      let widths: serde_json::Map<String, Value> = cells_of(*first_row)
        .iter()
        .enumerate()
        .filter_map(|(col, cell)| Some((col.to_string(), json!(style_px(cell, "width")?))))
        .collect();
      if !widths.is_empty() {
        data.insert(COLUMN_WIDTHS_FIELD.to_string(), Value::Object(widths));
      }
    }
    let table_id = self.add_block(BlockType::SimpleTable, parent, data);

    for (row_index, row) in rows.iter().enumerate() {
      let row_id = self.add_block(BlockType::SimpleTableRow, &table_id, BlockData::new());
      for (col_index, cell) in cells_of(*row).iter().enumerate() {
        let mut cell_data = BlockData::new();
        cell_data.insert(ROW_POSITION_FIELD.to_string(), json!(row_index));
        cell_data.insert(COL_POSITION_FIELD.to_string(), json!(col_index));
        let cell_id = self.add_block(BlockType::SimpleTableCell, &row_id, cell_data);
        let paragraph_id = self.add_block(BlockType::Paragraph, &cell_id, BlockData::new());
        self.fill_text_block(&paragraph_id, *cell, &vec![]);
      }
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Inline content
  // ---------------------------------------------------------------------------------------------

  fn inline_node(
    &mut self,
    node: ego_tree::NodeRef<Node>,
    attrs: &[(String, Value)],
    delta: &mut Delta,
  ) {
    match node.value() {
      Node::Text(text) => {
        let text: &str = text;
        if !text.is_empty() {
          delta.insert(text.to_string(), attrs.to_vec());
        }
      },
      Node::Element(_) => {
        let element = ElementRef::wrap(node).expect("element node");
        self.inline_element(element, attrs, delta);
      },
      _ => {},
    }
  }

  fn inline_element(&mut self, element: ElementRef, attrs: &[(String, Value)], delta: &mut Delta) {
    let mut attrs = attrs.to_vec();
    match element.value().name() {
      "br" => {
        delta.insert("\n".to_string(), attrs);
        return;
      },
      // Icons and checkboxes are not text.
      "img" | "input" | "script" | "style" => return,
      "strong" | "b" => attrs.push((BOLD_ATTR.to_string(), json!(true))),
      "em" | "i" => attrs.push((ITALIC_ATTR.to_string(), json!(true))),
      "u" => attrs.push((UNDERLINE_ATTR.to_string(), json!(true))),
      "del" | "s" => attrs.push((STRIKETHROUGH_ATTR.to_string(), json!(true))),
      "code" => attrs.push((CODE_ATTR.to_string(), json!(true))),
      "a" => {
        if let Some(href) = element.value().attr("href") {
          self.links.push(href.to_string());
          attrs.push((HREF_ATTR.to_string(), json!(href)));
        }
      },
      "mark" => {
        for class in element
          .value()
          .attr("class")
          .unwrap_or_default()
          .split_whitespace()
        {
          if let Some(color) = class.strip_prefix("highlight-") {
            if let Some(attr) = text_color_attr(color) {
              attrs.push(attr);
            }
          }
        }
      },
      "span" => {
        if has_class(element, "icon") {
          return;
        }
        if has_class(element, "user") {
          let name: String = element.text().collect();
          delta.insert(format!("@{}", name.trim()), attrs);
          return;
        }
      },
      _ => {},
    }

    for child in element.children() {
      self.inline_node(child, &attrs, delta);
    }
  }
}

// -------------------------------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------------------------------

fn cells_of(row: ElementRef<'_>) -> Vec<ElementRef<'_>> {
  row
    .children()
    .filter_map(ElementRef::wrap)
    .filter(|cell| matches!(cell.value().name(), "th" | "td"))
    .collect()
}

fn has_class(element: ElementRef, class: &str) -> bool {
  element
    .value()
    .attr("class")
    .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
}

fn has_inline_style(element: &ElementRef, property: &str) -> bool {
  element
    .value()
    .attr("style")
    .is_some_and(|style| style.contains(property))
}

/// The value in pixels of a property of the `style` attribute.
fn style_px(element: &ElementRef, property: &str) -> Option<f64> {
  element
    .value()
    .attr("style")?
    .split(';')
    .filter_map(|declaration| declaration.split_once(':'))
    .find(|(name, _)| name.trim() == property)?
    .1
    .trim()
    .strip_suffix("px")?
    .parse()
    .ok()
}

fn descendant_attr(element: ElementRef, tag: &str, attr: &str) -> Option<String> {
  element
    .descendants()
    .filter_map(ElementRef::wrap)
    .find(|descendant| descendant.value().name() == tag)
    .and_then(|descendant| descendant.value().attr(attr))
    .map(str::to_string)
}

fn is_inline_element(element: ElementRef) -> bool {
  matches!(
    element.value().name(),
    "a"
      | "span"
      | "strong"
      | "b"
      | "em"
      | "i"
      | "u"
      | "code"
      | "mark"
      | "del"
      | "s"
      | "br"
      | "time"
      | "img"
  )
}

/// Elements that are blocks of their own inside a text block, as opposed to its text.
fn is_nested_block(element: ElementRef) -> bool {
  matches!(
    element.value().name(),
    "ul"
      | "ol"
      | "details"
      | "figure"
      | "table"
      | "blockquote"
      | "aside"
      | "pre"
      | "hr"
      | "p"
      | "h1"
      | "h2"
      | "h3"
      | "h4"
      | "h5"
      | "h6"
  ) || (element.value().name() == "div"
    && (has_class(element, "column-list") || has_class(element, "collection-content")))
}

/// The line breaks around the content of a block come from the layout of the HTML.
fn trim_line_breaks(mut delta: Delta) -> Delta {
  while delta
    .ops
    .last()
    .is_some_and(|op| op.insert().chars().all(|c| c == '\n'))
  {
    delta.ops.pop();
  }
  delta
}

/// The color of a block, from its `block-color-*` class, as the data of the block and the
/// attributes of its text.
fn block_colors(element: ElementRef) -> (BlockData, Attributes) {
  let mut data = BlockData::new();
  let mut attrs = vec![];
  for class in element
    .value()
    .attr("class")
    .unwrap_or_default()
    .split_whitespace()
  {
    if let Some(color) = class.strip_prefix("block-color-") {
      if color.ends_with("_background") {
        if let Some(color) = notion_color(color, true) {
          data.insert(BLOCK_BG_COLOR_FIELD.to_string(), json!(color));
        }
      } else if let Some(color) = notion_color(color, false) {
        attrs.push((FONT_COLOR_ATTR.to_string(), json!(color)));
      }
    }
  }
  (data, attrs)
}

/// The attribute of a text colored by a `highlight-*` class.
fn text_color_attr(color: &str) -> Option<(String, Value)> {
  if color.ends_with("_background") {
    notion_color(color, true).map(|color| (BG_COLOR_ATTR.to_string(), json!(color)))
  } else {
    notion_color(color, false).map(|color| (FONT_COLOR_ATTR.to_string(), json!(color)))
  }
}

/// The color of Notion's palette, in the `0xAARRGGBB` format of AppFlowy. Returns `None` for the
/// default color.
fn notion_color(name: &str, background: bool) -> Option<&'static str> {
  let name = name.strip_suffix("_background").unwrap_or(name);
  Some(match (name, background) {
    ("gray", false) => "0xff787774",
    ("brown", false) => "0xff9f6b53",
    ("orange", false) => "0xffd9730d",
    ("yellow", false) => "0xffcb912f",
    ("teal" | "green", false) => "0xff448361",
    ("blue", false) => "0xff337ea9",
    ("purple", false) => "0xff9065b0",
    ("pink", false) => "0xffc14c8a",
    ("red", false) => "0xffd44c47",
    ("gray", true) => "0xfff1f1ef",
    ("brown", true) => "0xfff4eeee",
    ("orange", true) => "0xfffbecdd",
    ("yellow", true) => "0xfffbf3db",
    ("teal" | "green", true) => "0xffedf3ec",
    ("blue", true) => "0xffe7f3f8",
    ("purple", true) => "0xfff6f3f9",
    ("pink", true) => "0xfffaf1f5",
    ("red", true) => "0xfffdebec",
    _ => return None,
  })
}
