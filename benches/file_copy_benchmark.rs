use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rayon::prelude::*;
use tokio::runtime::Runtime;
use std::fs::{self, DirEntry, File};
use std::path::Path;
use std::time::Duration;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::io::copy;
use std::fs::OpenOptions;
use tokio::fs::{self as async_fs, File as asyncFile};
use tokio::io::{self as async_io, AsyncReadExt, AsyncWriteExt};
use criterion::async_executor::AsyncExecutor;

criterion_group!{
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(50));
    targets = file_copy_benchmark
}
criterion_main!(benches);


fn file_copy_benchmark(c: &mut Criterion) {
    let nvme_src_files = "D:\\Users\\Owner\\Desktop\\TestCopy\\Source";
    let nvme_dest_folder_base = "D:\\Users\\Owner\\Desktop\\TestCopy\\Destination";
    let hdd_src_files = "E:\\TestCopy\\Source";
    let hdd_dest_folder_base = "E:\\TestCopy\\Destination";


    let source_path = Path::new(nvme_src_files);

    // Define an array of (name, closure) tuples
    let variations = vec![
        ("default_copy", Box::new(|sp: &Path, dp: &Path| copy_files(sp, dp)) as Box<dyn Fn(&Path, &Path)>),
        ("buffer_size_copy", 
            Box::new(|sp: &Path, dp: &Path| 
            {
                let _ = copy_files_with_buffer_size(sp, dp, 8192);
            })
        ),
        ("block_size_copy", 
            Box::new(|sp: &Path, dp: &Path| 
            {
                let _ = copy_files_with_block_size(sp, dp);
            })
        ),
        ("chunks_copy", 
            Box::new(|sp: &Path, dp: &Path| 
            {
                let _ = copy_files_in_chunks(sp, dp, 1024 * 1024); 
            })
        ),
        ("write_through_copy", 
            Box::new(|sp: &Path, dp: &Path| 
            {
                 let _ = copy_files_write_through(sp, dp, 8192);
            })
        ),
    ];

    let async_variations = vec![
        ("async_chunks_4096", 4096),    // Example with a chunk size of 4096 bytes
        ("async_buffer_8192", 8192),    // Example with a buffer size of 8192 bytes
    ];
    // let mut group = c.benchmark_group("File Copy");

    /*         USE THIS ONE FOR NON-ASYNC BENCHES           */
    // for (name, func) in variations {
    //     let dest = format!("{}\\{}", hdd_dest_folder_base, name);
    //     let dest_path = Path::new(&dest);

    //     // Setup: Ensure the destination directory exists
    //     if dest_path.exists() {
    //         fs::remove_dir_all(&dest_path).unwrap(); // Clear existing directory
    //     }
    //     fs::create_dir_all(&dest_path).expect("Failed to create destination directory");

    //     // Define a benchmark for each function variant
    //     group.bench_with_input(BenchmarkId::new("Copy Method", name), &source_path, |b, sp| {
    //         b.iter(|| {
    //             func(black_box(&sp), black_box(&dest_path))
    //         })
    //     });
    //     // Teardown: Optionally clear the directory after each benchmark
    //     // fs::remove_dir_all(&dest_path).unwrap();
    // }
    /*         USE THIS ONE FOR ASYNC BENCHES           */
    let mut group = c.benchmark_group("File Copy Async Operations");
    group.measurement_time(Duration::from_secs(60));

    let rt = tokio::runtime::Builder::new_current_thread()
    .enable_io()
    .build()
    .unwrap();

    for (name, chunk_size) in async_variations {
        let x = format!("{}-{}", hdd_dest_folder_base, name);
        let dest_path = Path::new(&x);
        group.bench_with_input(BenchmarkId::new(name, chunk_size), &chunk_size, |b, &_chunk_size| {
            b.to_async(&rt).iter(|| async {
                async_copy_directory(&source_path, &dest_path, chunk_size).await.unwrap();
            });
        });
    }

    group.finish();
}

async fn async_copy_directory(src_dir: &Path, dst_dir: &Path, chunk_size: usize) -> io::Result<()> {
    tokio::fs::create_dir_all(dst_dir).await?; // Ensure destination directory exists

    let mut entries = tokio::fs::read_dir(src_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        if src_path.is_file() {  // Make sure it's a file and not a directory
            let dst_path = dst_dir.join(entry.file_name());
            async_copy_file(&src_path, &dst_path, chunk_size).await?;
        }
    }
    Ok(())
}

async fn async_copy_file(src: &Path, dst: &Path, chunk_size: usize) -> io::Result<()> {
    let mut src_file = tokio::fs::File::open(src).await?;
    let mut dst_file = tokio::fs::File::create(dst).await?;
    let mut buffer = vec![0; chunk_size];
    loop {
        let n = src_file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        dst_file.write_all(&buffer[..n]).await?;
    }
    Ok(())
}


fn copy_files(source_path: &Path, dest_path: &Path) {
    let entries = fs::read_dir(source_path).unwrap().collect::<Result<Vec<_>, std::io::Error>>().unwrap();

    entries.par_iter().for_each(|entry| {
        let path = entry.path();
        if path.is_file() {
            let dest_file_path = dest_path.join(entry.file_name());
            match fs::copy(&path, &dest_file_path) {
                Ok(_) => (),
                Err(e) => eprintln!("Failed to copy {:?} due to {:?}", path, e),
            }
        }
    });
}

fn copy_files_with_buffer_size(source_path: &Path, dest_path: &Path, buffer_size: usize) -> io::Result<()> {
    let source_file = File::open(source_path)?;
    let mut dest_file = File::create(dest_path)?;
    let mut reader = BufReader::with_capacity(buffer_size, source_file);
    let mut writer = BufWriter::with_capacity(buffer_size, dest_file);

    let mut buffer = vec![0; buffer_size];
    while let Ok(bytes_read) = reader.read(&mut buffer) {
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buffer[..bytes_read])?;
    }
    writer.flush()?;
    Ok(())
}

fn copy_files_with_block_size(source_path: &Path, dest_path: &Path) -> io::Result<()> {
    let block_size = 4096; // Typical block size for many filesystems
    copy_files_with_buffer_size(source_path, dest_path, block_size)
}


fn copy_files_in_chunks(source_path: &Path, dest_path: &Path, chunk_size: usize) -> io::Result<()> {
    let mut source_file = BufReader::with_capacity(chunk_size, File::open(source_path)?);
    let mut dest_file = BufWriter::with_capacity(chunk_size, File::create(dest_path)?);

    // Using std::io::copy with a limited buffer
    io::copy(&mut source_file, &mut dest_file)?;
    Ok(())
}


fn copy_files_write_through(source_path: &Path, dest_path: &Path, buffer_size: usize) -> io::Result<()> {
    let source_file = File::open(source_path)?;
    let dest_file = File::create(dest_path)?;
    let mut reader = BufReader::new(source_file);
    let mut writer = BufWriter::new(dest_file);

    let mut buffer = vec![0; buffer_size];
    while let Ok(bytes_read) = reader.read(&mut buffer) {
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buffer[..bytes_read])?;
        writer.flush()?;  // Ensuring immediate disk write
    }
    Ok(())
}
