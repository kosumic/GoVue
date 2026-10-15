use std::io::{self, BufRead};

fn main() {
    let directory = std::env::args()
        .nth(1)
        .expect("Usage: lingoplay-core-cli <cache-directory>");
    for line in io::stdin().lock().lines() {
        println!(
            "{}",
            lingoplay_core::execute(line.expect("Could not read request"), directory.clone())
        );
    }
}
