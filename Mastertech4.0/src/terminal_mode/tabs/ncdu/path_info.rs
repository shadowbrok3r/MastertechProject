use std::{collections::BTreeMap,env,ffi::OsString,fs, io,iter::FromIterator,path::{Path, PathBuf},sync::{Arc, Mutex},thread};

use sysinfo::System;

#[derive(Debug)]
pub enum PathInfo {
    File(u64),
    Folder(u64, BTreeMap<OsString, PathInfo>, usize),
}

impl PathInfo {
    fn size(&self) -> u64 {
        match *self {
            PathInfo::Folder(s, _, _) => s,
            PathInfo::File(s) => s,
        }
    }

    fn join(&mut self, vec: &Vec<OsString>) -> Result<&mut PathInfo, io::Error> {
        let mut curr_res = self;
        for comp in vec {
            match curr_res {
                PathInfo::Folder(_, c, ..) => {
                    if c.contains_key(comp) {
                        curr_res = c.get_mut(comp).unwrap();
                    } else {
                        return Err(io::Error::new(io::ErrorKind::Other, ""));
                    }
                }
                PathInfo::File(..) => return Err(io::Error::new(io::ErrorKind::Other, "")),
            };
        }
        match curr_res {
            PathInfo::Folder(..) => Ok(curr_res),
            PathInfo::File(..) => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }

    fn contents(&self) -> Result<&BTreeMap<OsString, PathInfo>, io::Error> {
        match self {
            PathInfo::Folder(_, c, ..) => Ok(c),
            PathInfo::File(..) => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }

    fn sorted(&self) -> Result<Vec<(&OsString, &PathInfo)>, io::Error> {
        match self {
            PathInfo::Folder(_, c, _) => {
                let mut contents_vec = Vec::from_iter(c.iter());
                contents_vec.sort_by(|(_, a), (_, b)| a.size().cmp(&b.size()).reverse());
                Ok(contents_vec)
            }
            _ => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }
}

pub fn join_path_to_vec(path: &Path, vec: Vec<OsString>) -> PathBuf {
    let mut tmp_path = path.to_path_buf();
    for comp in vec {
        tmp_path = tmp_path.join(comp);
    }
    tmp_path
}

pub fn get_starting_dir() -> Result<PathBuf, io::Error> {
    let current_dir = match env::current_dir() {
        Ok(dir) => dir,
        Err(e) => return Err(e),
    };
    let args = env::args().collect::<Vec<String>>();
    match args.len() {
        1 => Ok(current_dir),
        2 => Ok(PathBuf::from(&args[1])),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "")),
    }
}

pub fn get_wrapped_contents(dir: &Path) -> PathInfo {
    let threads = Arc::new(Mutex::new(1));
    let max_threads = System::new().physical_core_count().unwrap();
    let contents = get_contents(dir, threads, max_threads).unwrap();
    PathInfo::Folder(sum_contents(&contents), contents, 0)
}

pub fn get_contents(
    dir: &Path,
    threads: Arc<Mutex<usize>>,
    max_threads: usize,
) -> Result<BTreeMap<OsString, PathInfo>, io::Error> {
    let contents = Arc::new(Mutex::new(BTreeMap::new()));
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied => return Ok(BTreeMap::new()),
        Err(e) => panic!("{}", e),
    };

    let mut handlers = Vec::new();

    for entry in entries {
        let contents_clone = Arc::clone(&contents);
        // TODO: handle errors in threads
        let threads_depth_clone = Arc::clone(&threads);
        let task = move || {
            let safe_entry = entry.unwrap();
            let metadata = fs::symlink_metadata(safe_entry.path()).unwrap();
            if metadata.is_dir() {
                let sub_contents =
                    get_contents(&safe_entry.path(), threads_depth_clone, max_threads).unwrap();
                contents_clone.lock().unwrap().insert(
                    OsString::from(safe_entry.path().components().last().unwrap().as_os_str()),
                    PathInfo::Folder(
                        sum_contents(&sub_contents) + metadata.len(),
                        sub_contents,
                        0,
                    ),
                );
            } else {
                contents_clone.lock().unwrap().insert(
                    OsString::from(safe_entry.path().components().last().unwrap().as_os_str()),
                    PathInfo::File(metadata.len()),
                );
            };
        };
        if *threads.lock().unwrap() < max_threads {
            let threads_breadth_clone = Arc::clone(&threads);
            handlers.push(thread::spawn(move || {
                *threads_breadth_clone.lock().unwrap() += 1;
                task();
                *threads_breadth_clone.lock().unwrap() -= 1;
            }));
        } else {
            task();
        }
    }

    for handler in handlers {
        match handler.join() {
            Ok(_) => {}
            Err(_) => {}
        };
    }

    Ok(Arc::try_unwrap(contents).unwrap().into_inner().unwrap())
}

pub fn sum_contents(contents: &BTreeMap<OsString, PathInfo>) -> u64 {
    contents.values().fold(0, |acc, x| acc + x.size())
}

pub fn prettify_bytes(bytes: &u64) -> String {
    // Adapted from https://github.com/banyan/rust-pretty-bytes
    if bytes < &1024 {
        return bytes.to_string();
    }
    let float_bytes = *bytes as f64;
    let units = ["", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let exp = (float_bytes.ln() / 1024_f64.ln()).floor() as i32;
    format!(
        "{:.1}{}",
        float_bytes / 1024_f64.powi(exp),
        units[exp as usize]
    )
}

pub fn pad_and_prettify_bytes(bytes: &u64) -> String {
    let pretty_bytes = prettify_bytes(bytes);
    " ".repeat(8 - pretty_bytes.len()) + &pretty_bytes
}

pub fn size_bar(child_bytes: &u64, parent_bytes: &u64) -> String {
    let bar_components = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let fraction = *child_bytes as f64 / *parent_bytes as f64;
    let floored_frac = (fraction * 8_f64).floor().max(0_f64);
    let mut bar = "█".repeat(floored_frac as usize)
        + &bar_components[(((fraction - (floored_frac / 8_f64)) * 64_f64).round() as usize)
            .min(8)
            .max(0)]
        .to_string();
    bar += &" ".repeat((7 - floored_frac as usize).min(8));
    " [".to_string() + &bar + "] "
}