use super::*;

#[tokio::test]
async fn sqlite_catalog_reads_its_own_tables() {
    let conn = Mutex::new(Connection::open_in_memory().unwrap());
    conn.lock()
        .unwrap()
        .execute_batch(r#"CREATE TABLE "OrderSet" ("ID" TEXT, "Amount" INTEGER)"#)
        .unwrap();
    let catalog = SqliteCatalog::new(&conn);
    assert!(catalog.table_exists("OrderSet").await.unwrap());
    assert!(!catalog.table_exists("Nope").await.unwrap());
    let columns = catalog.existing_columns("OrderSet").await.unwrap();
    assert!(columns.contains("ID") && columns.contains("Amount"));
    assert!(catalog.existing_columns("Nope").await.unwrap().is_empty());
}
