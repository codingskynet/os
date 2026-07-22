use std::collections::BTreeMap;
use std::time::Duration;
use std::{fs, thread};

fn main() {
    let words = ["rust", "standard", "library"];
    let lengths: BTreeMap<_, _> = words.into_iter().map(|word| (word, word.len())).collect();

    println!("hello from Rust std: {lengths:?}");
    let contents = fs::read_to_string("/hello.txt").expect("failed to read /hello.txt");
    println!("read /hello.txt: {}", contents.trim_end());
    thread::sleep(Duration::from_millis(1));
    println!("Rust std sleep completed");
}
