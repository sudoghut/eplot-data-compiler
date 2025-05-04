//! Rust program to clone/pull a repo, extract info from markdown files, and save to SQLite.

use std::fs;
use std::path::Path;
use std::process::Command;
use regex::Regex;
use rusqlite::{params, Connection, Result};

fn main() -> Result<()> {
    let repo_url = "https://github.com/sudoghut/eplot";
    let repo_dir = "eplot";

    // Clone or pull the repo
    if Path::new(repo_dir).exists() {
        println!("Repo exists, running git pull...");
        let status = Command::new("git")
            .arg("-C")
            .arg(repo_dir)
            .arg("pull")
            .status()
            .expect("Failed to run git pull");
        if !status.success() {
            eprintln!("git pull failed");
            return Ok(());
        }
    } else {
        println!("Cloning repo...");
        let status = Command::new("git")
            .arg("clone")
            .arg(repo_url)
            .status()
            .expect("Failed to run git clone");
        if !status.success() {
            eprintln!("git clone failed");
            return Ok(());
        }
    }

    // Find first 5 markdown files in eplot/src/content/blog
    let blog_dir = format!("{}/src/content/blog", repo_dir);
    let mut md_files: Vec<_> = fs::read_dir(&blog_dir)
        .expect("Failed to read blog dir")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "md" { Some(path) } else { None }
        })
        .collect();
    md_files.sort();
    // md_files.truncate(5);

    // Prepare regex for tags
    let tag_re = Regex::new(r"tags:\s*\[([^\]]*)\]").unwrap();
    let yyyymm_re = Regex::new(r"\d{6}").unwrap();

    // Open SQLite connection
    let conn = Connection::open("data.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS data (
            ep_name TEXT,
            ep_num TEXT,
            ep_year TEXT,
            ep_month TEXT
        )",
        [],
    )?;
    // Empty the data table before inserting new data
    conn.execute("DELETE FROM data", [])?;
    conn.execute("DELETE FROM sqlite_sequence WHERE name='data'", [])?;

    for path in md_files {
        let filename = path.file_name().unwrap().to_string_lossy();
        let parts: Vec<&str> = filename.split('_').collect();
        let (ep_name, ep_num) = if parts.len() >= 2 {
            (parts[0].to_string(), parts[1].trim_end_matches(".md").to_string())
        } else {
            (filename.to_string(), "".to_string())
        };

        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut ep_year = String::new();
        let mut ep_month = String::new();

        if let Some(tag_caps) = tag_re.captures(&content) {
            if let Some(tags_str) = tag_caps.get(1) {
                if let Some(yyyymm) = yyyymm_re.find(tags_str.as_str()) {
                    let yyyymm = yyyymm.as_str();
                    if yyyymm.len() == 6 {
                        ep_year = yyyymm[0..4].to_string();
                        ep_month = yyyymm[4..6].to_string();
                    }
                }
            }
        }

        println!("Inserting: {}, {}, {}, {}", ep_name, ep_num, ep_year, ep_month);
        conn.execute(
            "INSERT INTO data (ep_name, ep_num, ep_year, ep_month) VALUES (?1, ?2, ?3, ?4)",
            params![ep_name, ep_num, ep_year, ep_month],
        )?;
    }

    println!("Done.");
    Ok(())
}
