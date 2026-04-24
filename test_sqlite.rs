use sqlx::{SqlitePool, Row};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing SQLite connection...");
    
    let pool = SqlitePool::connect("sqlite:/tmp/relay_queue.db").await?;
    println!("Connected successfully!");
    
    let result = sqlx::query("SELECT 1 as test")
        .fetch_one(&pool)
        .await?;
    
    let test_val: i32 = result.get("test");
    println!("Query result: {}", test_val);
    
    Ok(())
}
