//! Rust program to clone/pull a repo, extract info from markdown files, and save to SQLite.

use std::fs;
use std::path::Path;
use std::process::Command;
use regex::Regex;
use rusqlite::{params, Connection, Result};

// Helper function to clean series names by removing episode numbers
fn clean_series_name(name: &str) -> String {
    if let Some(last_space_idx) = name.rfind(' ') {
        let (base_name, episode_part) = name.split_at(last_space_idx);
        let ep_part = episode_part.trim();
        if ep_part.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return base_name.trim().to_string();
        }
    }
    name.to_string()
}

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

    // Prepare regex patterns
    let tag_re = Regex::new(r"tags:\s*\[([^\]]*)\]").unwrap();
    let yyyymm_re = Regex::new(r"\d{6}").unwrap();
    let title_re = Regex::new(r#"title:\s*"([^"]*)""#).unwrap();

    // Open SQLite connection
    let conn = Connection::open("data.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS series_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            series_name TEXT UNIQUE,
            series_year TEXT,
            series_month TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ep_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ep_name TEXT,
            ep_num TEXT,
            ep_year TEXT,
            ep_month TEXT,
            series_id INTEGER,
            abstract TEXT
        )",
        [],
    )?;
    // Empty the tables before inserting new data
    conn.execute("DELETE FROM ep_data", [])?;
    conn.execute("DELETE FROM sqlite_sequence WHERE name='ep_data'", [])?;
    conn.execute("DELETE FROM series_data", [])?;
    conn.execute("DELETE FROM sqlite_sequence WHERE name='series_data'", [])?;

    use std::collections::HashMap;
    // First pass: collect unique series and their year
    let mut series_map: HashMap<String, (String, String)> = HashMap::new();
    let mut episodes: Vec<(String, String, String, String, String)> = Vec::new();
    let desc_re = Regex::new(r#"description:\s*["']?([^"\n']*)"#).unwrap();
    for path in &md_files {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let filename = path.file_name().unwrap().to_string_lossy();
        let parts: Vec<&str> = filename.split('_').collect();
        let (series_name, ep_num) = if parts.len() >= 2 {
            // Get title from markdown or filename
            let full_title = if let Some(title_caps) = title_re.captures(&content) {
                title_caps.get(1).map_or(parts[0].to_string(), |m| m.as_str().to_string())
            } else {
                parts[0].to_string()
            };
            
            // Clean the series name by removing episode number if present at end
            let clean_name = clean_series_name(&full_title);
            
            (clean_name, parts[1].trim_end_matches(".md").to_string())
        } else {
            (filename.to_string(), "".to_string())
        };

        let mut ep_year = String::new();
        let mut ep_month = String::new();
        let mut abstract_text = String::new();

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

        if let Some(desc_caps) = desc_re.captures(&content) {
            abstract_text = desc_caps.get(1).map_or(String::new(), |m| m.as_str().trim().to_string());
        }
        if abstract_text.is_empty() {
            // Find content after the second '---'
            let mut lines = content.lines();
            let mut dash_count = 0;
            let mut below = String::new();
            while let Some(line) = lines.next() {
                if line.trim() == "---" {
                    dash_count += 1;
                    if dash_count == 2 {
                        break;
                    }
                }
            }
            // Collect the rest of the lines as content
            for line in lines {
                below.push_str(line.trim());
                below.push(' ');
            }
            let below = below.trim();
            let below_chars: String = below.chars().take(200).collect();
            if below.chars().count() > 200 {
                abstract_text = format!("{}...", below_chars);
            } else {
                abstract_text = below_chars;
            }
        }

        // Store series info with clean name
        series_map.entry(series_name.clone()).or_insert((ep_year.clone(), ep_month.clone()));
        // Store full episode info with clean name reference
        episodes.push((series_name.clone(), ep_num, ep_year, ep_month, abstract_text));
    }

    // Insert unique series into series_data (names are already cleaned)
    for (series_name, (series_year, series_month)) in &series_map {
        conn.execute(
            "INSERT INTO series_data (series_name, series_year, series_month) VALUES (?1, ?2, ?3)",
            params![series_name, series_year, series_month],
        )?;
    }

    // Insert episodes with correct series_id
    for (series_name, ep_num, ep_year, ep_month, abstract_text) in episodes {
        let clean_name = clean_series_name(&series_name);
        let mut stmt = conn.prepare("SELECT id FROM series_data WHERE series_name = ?1")?;
        let series_id: i64 = stmt.query_row(params![clean_name], |row| row.get(0))?;
        println!("Inserting: {}, {}, {}, {}, series_id={}, abstract={}", series_name, ep_num, ep_year, ep_month, series_id, abstract_text);
        conn.execute(
            "INSERT INTO ep_data (ep_name, ep_num, ep_year, ep_month, series_id, abstract) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![series_name, ep_num, ep_year, ep_month, series_id, abstract_text],
        )?;
    }

    println!("Done.");
    Ok(())
}
