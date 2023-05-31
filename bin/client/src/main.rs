fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .env()
        .init()
        .unwrap();

    let mut db = sqlsync::Database::new();
    db.run(|tx| {
        tx.execute(
            "CREATE TABLE IF NOT EXISTS foo (id INTEGER PRIMARY KEY, name TEXT)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    db.branch();

    db.run(|tx| {
        tx.execute("INSERT INTO foo (name) VALUES (?)", ["hello"])?;
        tx.execute("INSERT INTO foo (name) VALUES (?)", ["world"])?;
        Ok(())
    })
    .unwrap();

    db.run(|tx| {
        let mut stmt = tx.prepare("select id, name from foo")?;
        let mut rows = stmt.query([])?;
        println!("rows:");
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            println!("\tid = {}, name = {}", id, name);
        }
        Ok(())
    })
    .unwrap();

    db.rollback();
    db.branch();

    db.run(|tx| {
        tx.execute("INSERT INTO foo (name) VALUES (?)", ["world"])?;
        let mut stmt = tx.prepare("select id, name from foo")?;
        let mut rows = stmt.query([])?;
        println!("rows:");
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            println!("\tid = {}, name = {}", id, name);
        }
        Ok(())
    })
    .unwrap();

    db.rollback();

    db.run(|tx| {
        tx.execute("INSERT INTO foo (name) VALUES (?)", ["hi"])?;
        tx.execute("INSERT INTO foo (name) VALUES (?)", ["bob"])?;
        let mut stmt = tx.prepare("select id, name from foo")?;
        let mut rows = stmt.query([])?;
        println!("rows:");
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            println!("\tid = {}, name = {}", id, name);
        }
        Ok(())
    })
    .unwrap();
}
