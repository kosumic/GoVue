use std::io::{self, BufRead};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("Usage: govue-core-cli <cache-directory>")?;
    for line in io::stdin().lock().lines() {
        println!("{}", govue_core::execute(line?, directory.clone()));
    }
    Ok(())
}
