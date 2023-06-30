use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn copy_file(src_path: &str, dst_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Open source file.
    let mut src_file = File::open(src_path).await?;
    let mut dst_file = File::create(dst_path).await?;
    
    // Create buffer to read into.
    let mut buffer = Vec::new();

    // Read file to string.
    src_file.read_to_end(&mut buffer).await?;
    
    // Write to destination file
    dst_file.write_all(&buffer).await?;

    Ok(())
}

// Spawning the copy task
//let copy_task = tokio::spawn(copy_file("src.txt", "dst.txt"));
