use collab::document::blocks::DocumentData;
use collab::importer::notion::NotionImporter;
use collab::importer::notion::html::{PageIcon, parse_notion_html};
use std::path::Path;

const HOME_ID: &str = "11111111111111111111111111111111";
const FIRST_ID: &str = "22222222222222222222222222222222";
const SECOND_ID: &str = "33333333333333333333333333333333";

fn write(path: &Path, content: &str) {
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, content).unwrap();
}

fn page(icon: &str, title: &str, body: &str) -> String {
  format!(
    r#"<html><head><style>.page-body {{ color: red; }}</style></head><body>
<article id="a" class="page sans" data-notion-page-icon="{icon}"><header><h1 class="page-title">{title}</h1></header>
<div class="page-body">{body}</div></article></body></html>"#
  )
}

/// The types and texts of the children of a block, in order.
fn children(data: &DocumentData, id: &str) -> Vec<(String, String)> {
  data
    .meta
    .children_map
    .get(id)
    .into_iter()
    .flatten()
    .map(|child| {
      let text = data
        .meta
        .text_map
        .as_ref()
        .and_then(|map| map.get(child))
        .cloned()
        .unwrap_or_default();
      (data.blocks[child].ty.clone(), text)
    })
    .collect()
}

fn child_ids<'a>(data: &'a DocumentData, id: &str) -> Vec<&'a String> {
  data
    .meta
    .children_map
    .get(id)
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn page_header_has_icon_and_cover() {
  let html = r#"<body><article data-notion-page-icon="📓"><header>
<img class="page-cover-image" src="https://example.com/cover.jpg"/>
<h1 class="page-title">Title</h1></header><div class="page-body"><p>Hello</p></div></article></body>"#;
  let page = parse_notion_html("root", html);
  assert_eq!(page.icon, Some(PageIcon::Emoji("📓".to_string())));
  assert_eq!(page.cover.as_deref(), Some("https://example.com/cover.jpg"));
  assert!(page.has_content);

  let page = parse_notion_html("root", "<body><article><header></header></article></body>");
  assert_eq!(page.icon, None);
  assert!(!page.has_content);
}

#[test]
fn blocks_keep_their_colors_and_layout() {
  let body = r#"
<div class="column-list">
  <div class="column" data-notion-column-ratio="0.25"><h2 class="block-color-blue_background">Left</h2></div>
  <div class="column" data-notion-column-ratio="0.75"><p>Right <mark class="highlight-red">red</mark></p></div>
</div>
<aside class="callout" data-notion-callout-icon="💡" data-notion-callout-background="gray_background"><div style="font-size:1.5em"><span class="icon"></span></div><div style="width:100%"><p>Note</p></div></aside>
<ul class="to-do-list"><li><input type="checkbox" class="checkbox checkbox-on" checked/> <span class="to-do-children-checked">done</span></li></ul>
<ol class="numbered-list" start="3"><li>third<ul class="bulleted-list"><li>nested</li></ul></li></ol>
<details class="toggle"><summary>More</summary><div class="indented"><p>hidden</p></div></details>
<hr/>"#;
  let page = parse_notion_html("root", &page("", "T", body));
  let data = page.document;

  let top = children(&data, "root");
  let types: Vec<_> = top.iter().map(|(ty, _)| ty.as_str()).collect();
  assert_eq!(
    types,
    [
      "simple_columns",
      "callout",
      "todo_list",
      "numbered_list",
      "toggle_list",
      "divider"
    ]
  );

  // Columns keep their ratio and hold their blocks.
  let columns_id = child_ids(&data, "root")[0].clone();
  let columns = child_ids(&data, &columns_id);
  assert_eq!(columns.len(), 2);
  assert_eq!(data.blocks[columns[0]].data["ratio"], 0.25);
  assert_eq!(data.blocks[columns[1]].data["ratio"], 0.75);
  let heading = child_ids(&data, columns[0])[0];
  assert_eq!(data.blocks[heading].ty, "heading");
  assert_eq!(data.blocks[heading].data["bgColor"], "0xffe7f3f8");
  let paragraph = child_ids(&data, columns[1])[0];
  let text = data.meta.text_map.as_ref().unwrap()[paragraph].clone();
  assert!(text.contains(r#""font_color":"0xffd44c47""#), "{text}");

  // A callout has its icon, color and text.
  let callout = child_ids(&data, "root")[1];
  assert_eq!(data.blocks[callout].data["icon"], "💡");
  assert_eq!(data.blocks[callout].data["bgColor"], "0xfff1f1ef");
  assert_eq!(
    data.meta.text_map.as_ref().unwrap()[callout],
    r#"[{"insert":"Note"}]"#
  );

  // The text of a to-do item does not start with the space after its checkbox.
  let todo = child_ids(&data, "root")[2];
  assert_eq!(data.blocks[todo].data["checked"], true);
  assert_eq!(
    data.meta.text_map.as_ref().unwrap()[todo],
    r#"[{"insert":"done"}]"#
  );

  // A numbered list starts at its number and holds its nested list.
  let numbered = child_ids(&data, "root")[3];
  assert_eq!(data.blocks[numbered].data["number"], 3);
  assert_eq!(children(&data, numbered).len(), 1);

  // A toggle holds the blocks of its body.
  let toggle = child_ids(&data, "root")[4];
  assert_eq!(children(&data, toggle)[0].0, "paragraph");
}

#[test]
fn simple_table_has_its_header_and_widths() {
  let body = r#"<table class="simple-table"><tbody>
<tr><th style="width:100px">A</th><th style="width:50px">B</th></tr>
<tr><td>1</td><td>2</td></tr></tbody></table>"#;
  let page = parse_notion_html("root", &page("", "T", body));
  let data = page.document;
  let table = child_ids(&data, "root")[0];
  assert_eq!(data.blocks[table].ty, "simple_table");
  assert_eq!(data.blocks[table].data["enable_header_row"], true);
  assert!(!data.blocks[table].data.contains_key("enable_header_column"));
  assert_eq!(data.blocks[table].data["column_widths"]["0"], 100.0);
  assert_eq!(data.meta.children_map[table].len(), 2);
}

/// A page with sub pages of a HTML export is a page with the icon of its page, and its sub pages
/// follow the order of its links.
#[tokio::test]
async fn html_export_keeps_icons_and_order() {
  let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
  let space = root.join("export").join("Private & Shared");
  let link = |name: &str, id: &str| {
    format!(
      r#"<figure class="link-to-page"><a href="Home/{name}%20{id}.html"><span class="icon" data-emoji="🔗"></span>{name}</a></figure>"#
    )
  };
  write(
    &space.join(format!("Home {HOME_ID}.html")),
    &page(
      "🏠",
      "Home",
      &format!("{}{}", link("Second", SECOND_ID), link("First", FIRST_ID)),
    ),
  );
  write(
    &space.join("Home").join(format!("First {FIRST_ID}.html")),
    &page("1️⃣", "First", "<p>one</p>"),
  );
  write(
    &space.join("Home").join(format!("Second {SECOND_ID}.html")),
    &page("", "Second", "<p>two</p>"),
  );

  let importer = NotionImporter::new(
    1,
    root.join("export"),
    uuid::Uuid::new_v4(),
    "http://test.appflowy.cloud".to_string(),
  )
  .unwrap();
  let info = importer.import().await.unwrap();

  let space = &info.views()[0];
  assert_eq!(space.children.len(), 1);
  let home = &space.children[0];
  assert_eq!(home.notion_name, "Home");
  let names: Vec<_> = home
    .children
    .iter()
    .map(|c| c.notion_name.as_str())
    .collect();
  assert_eq!(names, ["Second", "First"]);

  // The links to the sub pages become mentions of them.
  let (document, _) = home.as_document().await.unwrap();
  let data = document.get_document_data().unwrap();
  let mentions = child_ids(&data, &data.page_id)
    .iter()
    .filter(|id| data.blocks[**id].data.contains_key("view_id"))
    .count();
  assert_eq!(mentions, 2);

  let nested = info.build_nested_views().await;
  let _ = std::fs::remove_dir_all(&root);
  let home_view = &nested.views[0].children[0];
  assert_eq!(
    home_view.view.icon.as_ref().map(|icon| icon.value.as_str()),
    Some("🏠")
  );
  assert_eq!(
    home_view.children[1]
      .view
      .icon
      .as_ref()
      .map(|icon| icon.value.as_str()),
    Some("1️⃣")
  );
  assert!(home_view.children[0].view.icon.is_none());
}

#[test]
fn notion_icons_are_replaced_by_emojis() {
  let page = |icon: &str| {
    parse_notion_html(
      "root",
      &format!(
        r#"<body><article data-notion-page-icon="{icon}"><header></header></article></body>"#
      ),
    )
    .icon
  };
  assert_eq!(
    page("/icons/bug_gray.svg"),
    Some(PageIcon::Emoji("🐞".to_string()))
  );
  assert_eq!(
    page("https://app.notion.com/icons/heart-rate_lightgray.svg"),
    Some(PageIcon::Emoji("💓".to_string()))
  );
  // An icon without an emoji for it is left out, and an image of the export is kept.
  assert_eq!(page("/icons/unknown-shape_gray.svg"), None);
  assert_eq!(
    page("Page/icon.png"),
    Some(PageIcon::Image("Page/icon.png".to_string()))
  );
}

/// A database that a page shows becomes a link to the database, and an icon that is an image of
/// the export is uploaded.
#[tokio::test]
async fn inline_database_and_image_icon() {
  let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
  let space = root.join("export").join("Private & Shared");
  let database = |title: &str| {
    format!(
      r#"<div class="collection-content" dir="ltr"><h4 class="collection-title">{title}</h4><div class="collection-content-wrapper"><table class="collection-content"><thead><tr><th>Name</th></tr></thead><tbody><tr><td>a</td></tr></tbody></table></div></div>"#
    )
  };
  write(
    &space.join(format!("Home {HOME_ID}.html")),
    &page(
      "Home/icon.png",
      "Home",
      &format!("<p>before</p>{}<p>after</p>", database("Tasks")),
    ),
  );
  write(
    &space.join("Home").join(format!("First {FIRST_ID}.html")),
    &page("", "First", "<p>one</p>"),
  );
  write(
    &space.join(format!("Tasks {SECOND_ID}.csv")),
    "Name,Status\na,Done\nb,Todo\n",
  );
  std::fs::write(space.join("Home").join("icon.png"), [137, 80, 78, 71]).unwrap();

  let importer = NotionImporter::new(
    1,
    root.join("export"),
    uuid::Uuid::new_v4(),
    "http://test.appflowy.cloud".to_string(),
  )
  .unwrap();
  let info = importer.import().await.unwrap();

  let home = info.views()[0]
    .children
    .iter()
    .find(|view| view.notion_name == "Home")
    .unwrap();
  let tasks = info.views()[0]
    .children
    .iter()
    .find(|view| view.notion_name == "Tasks")
    .unwrap();

  let (document, resource) = home.as_document().await.unwrap();
  let data = document.get_document_data().unwrap();
  let blocks = child_ids(&data, &data.page_id);
  assert_eq!(blocks.len(), 3);
  let linked = &data.blocks[blocks[1]];
  assert_eq!(linked.ty, "grid");
  assert_eq!(linked.data["view_id"], tasks.view_id);
  assert_eq!(linked.data["parent_id"], home.view_id);
  assert!(!linked.data.contains_key("notion_database"));
  let content = tasks.as_database().await.unwrap();
  assert_eq!(
    linked.data["database_id"],
    content.resource.object_id.to_string()
  );
  assert_eq!(resource.files.len(), 1);
  assert!(resource.files[0].ends_with("icon.png"));

  let nested = info.build_nested_views().await;
  let _ = std::fs::remove_dir_all(&root);
  let home_view = nested.views[0]
    .children
    .iter()
    .find(|view| view.view.name == "Home")
    .unwrap();
  let icon = home_view.view.icon.as_ref().unwrap();
  assert!(icon.value.starts_with("http://test.appflowy.cloud/"));
}

#[test]
fn bookmarks_are_link_previews_and_toggles_start_closed() {
  let page = parse_notion_html(
    "root",
    r#"<body><article><div class="page-body">
<figure><a href="https://example.com/a" class="bookmark source"><div class="bookmark-info"><div class="bookmark-title">A</div></div></a></figure>
<details open="" class="toggle"><summary>More</summary><div class="indented"><p>inside</p></div></details>
</div></article></body>"#,
  );
  let data = page.document;
  let blocks = child_ids(&data, "root");
  assert_eq!(blocks.len(), 2);
  assert_eq!(data.blocks[blocks[0]].ty, "link_preview");
  assert_eq!(data.blocks[blocks[0]].data["url"], "https://example.com/a");
  assert_eq!(data.blocks[blocks[1]].data["collapsed"], true);
}

#[test]
fn cover_offset_comes_from_the_position_of_the_image() {
  let header = collab::importer::notion::html::parse_notion_html_header(
    r#"<body><article><header><img class="page-cover-image" src="https://example.com/c.jpg" style="object-position:center 25%"/></header></article></body>"#,
  );
  assert_eq!(header.cover.as_deref(), Some("https://example.com/c.jpg"));
  assert_eq!(header.cover_offset, Some(-0.5));
}

#[test]
fn database_columns_follow_the_table_and_their_cells_tell_their_kind() {
  use collab::importer::notion::html::ColumnKind;
  let columns = collab::importer::notion::html::parse_notion_html_columns(
    r#"<table class="collection-content"><thead><tr>
<th><span class="icon"><img src="https://app.notion.com/icons/font_gray.svg"/></span>Name</th>
<th><span class="icon"><img src="https://app.notion.com/icons/calendar_gray.svg"/></span>Date</th>
<th><span class="icon"><img src="https://app.notion.com/icons/arrow-circle-down_gray.svg"/></span>Status</th>
<th><span class="icon"><img src="https://app.notion.com/icons/list_gray.svg"/></span>Tags</th>
<th><span class="icon"><img src="https://app.notion.com/icons/description_gray.svg"/></span>Notes</th></tr></thead>
<tbody><tr><td class="cell-title">a</td><td><time datetime="2025-01-02">January 2, 2025</time></td><td><span class="selected-value">Done</span></td><td><span class="selected-value">x</span></td><td>text</td></tr>
<tr><td class="cell-title">b</td><td><time datetime="2025-01-03">January 3, 2025</time></td><td><span class="selected-value">Todo</span></td><td><span class="selected-value">y</span></td><td></td></tr></tbody></table>"#,
  );
  let summary: Vec<_> = columns
    .iter()
    .map(|column| (column.name.as_str(), column.kind))
    .collect();
  assert_eq!(
    summary,
    [
      ("Name", None),
      ("Date", Some(ColumnKind::Date)),
      ("Status", Some(ColumnKind::Select)),
      ("Tags", Some(ColumnKind::MultiSelect)),
      ("Notes", None),
    ]
  );
}
