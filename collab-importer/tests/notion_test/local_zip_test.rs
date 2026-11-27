use collab_importer::notion::NotionImporter;
use collab_importer::zip_tool::sync_zip::sync_unzip;
use futures::StreamExt;

#[tokio::test]
#[ignore]
async fn print_imported_collabs() {
  let zip = std::env::var("NOTION_ZIP").expect("NOTION_ZIP");
  let out = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
  std::fs::create_dir_all(&out).unwrap();
  let dir = sync_unzip(zip.into(), out, Some("export".to_string()))
    .unwrap()
    .unzip_dir;
  let importer = NotionImporter::new(
    1,
    &dir,
    uuid::Uuid::new_v4(),
    "http://test.appflowy.cloud".to_string(),
  )
  .unwrap();
  let info = importer.import().await.unwrap();
  let mut stream = info.into_collab_stream().await;
  let mut n = 0;
  while let Some(collab) = stream.next().await {
    n += 1;
    println!("{} collabs={}", collab.name, collab.imported_collabs.len());
  }
  println!("TOTAL {n}");
}
