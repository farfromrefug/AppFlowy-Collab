use collab::importer::notion::NotionImporter;
use std::path::Path;

fn write(path: &Path, content: &str) {
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, content).unwrap();
}

const PARENT_ID: &str = "11111111111111111111111111111111";
const ALPHA_ID: &str = "22222222222222222222222222222222";
const BETA_ID: &str = "33333333333333333333333333333333";
const GAMMA_ID: &str = "44444444444444444444444444444444";

/// Notion lists the sub pages of a page in the order of its links, which is also the order of
/// its sidebar, while the files of the sub pages come in alphabetical order.
#[tokio::test]
async fn sub_pages_follow_the_order_of_the_links_in_the_parent_page() {
  let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
  let space = root.join("export").join("Private & Shared");
  write(&space.join(format!("Parent {PARENT_ID}.md")), "# Parent\n");
  let dir = space.join("Parent");
  write(&dir.join(format!("Alpha {ALPHA_ID}.md")), "# Alpha\n");
  write(&dir.join(format!("Beta {BETA_ID}.md")), "# Beta\n");
  write(&dir.join(format!("Gamma {GAMMA_ID}.md")), "# Gamma\n");
  // The parent page links its sub pages as Gamma, Alpha, Beta, and does not link to a page that
  // is only in the directory.
  write(
    &space.join(format!("Parent {PARENT_ID}.md")),
    &format!(
      "# Parent\n\n[Gamma](Parent/Gamma%20{GAMMA_ID}.md)\n\n[Alpha](Parent/Alpha%20{ALPHA_ID}.md)\n\n[Beta](Parent/Beta%20{BETA_ID}.md)\n"
    ),
  );
  write(
    &dir.join("Unlinked 55555555555555555555555555555555.md"),
    "# Unlinked\n",
  );

  let importer = NotionImporter::new(
    1,
    root.join("export"),
    uuid::Uuid::new_v4(),
    "http://test.appflowy.cloud".to_string(),
  )
  .unwrap();
  let info = importer.import().await.unwrap();
  let _ = std::fs::remove_dir_all(&root);

  // One space holding the parent page, which is not repeated as a space of its own.
  assert_eq!(info.views().len(), 1);
  let space = &info.views()[0];
  assert_eq!(space.notion_name, "Private & Shared");
  assert_eq!(space.children.len(), 1);
  let parent = &space.children[0];
  assert_eq!(parent.notion_name, "Parent");
  assert_eq!(parent.notion_id.as_deref(), Some(PARENT_ID));
  let names: Vec<_> = parent
    .children
    .iter()
    .map(|c| c.notion_name.as_str())
    .collect();
  assert_eq!(names, ["Gamma", "Alpha", "Beta", "Unlinked"]);
}
