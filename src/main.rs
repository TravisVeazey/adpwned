use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, BufWriter, Write};

mod consts;

struct User {
    rid: usize,
    username: String,
    password: String,
    uac: u32,
}

impl User {
    fn is_active(&self) -> bool {
        self.uac & consts::UAC_ACCOUNT_DISABLE == 0
    }
}

/// Find `hash` in a file of "pwned" password hashes
///
/// This function performs a binary search to perform the search in `O(log(n))` complexity,
/// utilizing [`Seek`] to scan through the file without reading the entire contents into memory.
///
/// `find_hash` will return an [`Option`] containing the number of times the hashed password has
/// been seen in breaches, or `None` if it has not been seen.
///
/// # Authorship
///
/// The core logic of this function was generated by ChatGPT, though it has since been fixed,
/// optimized, and specialized to this specific purpose by myself.
fn find_hash<R: BufRead + Seek>(reader: &mut R, hash: &str) -> Option<usize> {
    let mut left = 0;
    let mut right = reader.seek(SeekFrom::End(0)).unwrap();

    let target = hash.to_string();
    let mut line = String::new();

    while left <= right {
        let mid = left + (right - left) / 2;
        reader.seek(SeekFrom::Start(mid)).unwrap();
        reader.read_line(&mut line).unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();

        let split_line: Vec<_> = line.trim().split(':').collect();

        match split_line[0].cmp(&target) {
            std::cmp::Ordering::Equal => return Some(split_line[1].parse().unwrap()),
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid - 1,
        }
    }

    None
}

/// Use a jump search to progressively search through the file for sorted hashes
///
/// Based on the algorithm described at https://www.geeksforgeeks.org/jump-search/
fn jump_search<R: BufRead + Seek>(reader: &mut R, hash: &str) -> Option<usize> {
    let start = reader.stream_position().unwrap();
    let n = reader.seek(SeekFrom::End(0)).unwrap();
    let step = ((n as f32).sqrt() as u64).max(1);

    let mut line = String::new();

    reader.seek(SeekFrom::Start(start)).unwrap();

    loop {
        if reader.read_line(&mut line).unwrap() == 0 {
            return None; // Reached the end of the file without finding our target
        }

        let split_line: Vec<_> = line.trim().split(':').collect();

        match split_line[0].cmp(hash) {
            std::cmp::Ordering::Equal => return Some(split_line[1].parse().unwrap()),
            std::cmp::Ordering::Less => { },
            std::cmp::Ordering::Greater => break,
        }

        reader.seek(SeekFrom::Current(step as i64)).unwrap();
        reader.read_line(&mut line).unwrap();
        line.clear();
    }

    // Found the segment where our hash may be, start looking for it linearly
    let stop_at = reader.stream_position().unwrap();
    // Start by backing up to the start of the segment, then read the maybe-partial line
    reader.seek(SeekFrom::Current(0 - step as i64)).unwrap();
    reader.read_line(&mut line).unwrap();
    while reader.stream_position().unwrap() < stop_at {
        line.clear();
        reader.read_line(&mut line).unwrap();

        let split_line: Vec<_> = line.trim().split(':').collect();

        match split_line[0].cmp(hash) {
            std::cmp::Ordering::Equal => return Some(split_line[1].parse().unwrap()),
            std::cmp::Ordering::Less => { },
            std::cmp::Ordering::Greater => break, // Overshot our target, means it's not here
        }
    }

    None
}


fn main() {
    let start = std::time::Instant::now();

    let hashes = File::open("../hash.csv").expect("Unable to open hashes file");
    let hashreader = BufReader::new(hashes);

    let mut total_users = 0;
    let mut users: Vec<_> = hashreader.lines().flat_map(|line| {
        let line = line.ok()?;
        let split: Vec<_> = line.split_whitespace().collect();

        total_users += 1;
        
        Some(User {
            rid: split[0].parse().expect("Failed to parse RID: {line}"),
            username: split[1].to_string(),
            password: split[2].to_ascii_uppercase(),
            uac: split[3].parse().expect("Failed to parse userAccountControl: {line}"),
        })
    }).filter(|user| user.is_active()).collect();
    users.sort_unstable_by(|a, b| a.password.cmp(&b.password));
    let active_users = users.len();

    let file = File::open("../pwned-passwords-ntlm-ordered-by-hash-v8.txt").expect("Unable to open pwned passwords file");
    let mut reader = BufReader::new(file);

    let outfile = File::create("pwned.csv").expect("Unable to created output file");
    let mut writer = BufWriter::new(outfile);

    writeln!(&mut writer, "RID\tUser\tuserAccountControl\tPwned").expect("Failed to write to file");

    let mut pwned_users = 0;

    let mut last_hash = String::new();
    let mut last_pwned = 0;

    for (user, pwned) in users.iter().filter_map(|user| {
        if user.password == last_hash {
            pwned_users += 1;
            return Some((user, last_pwned));
        }
        last_hash = user.password.clone();

        last_pwned = jump_search(&mut reader, user.password.as_str())?;
        pwned_users += 1;
        Some((user, last_pwned))
    })
    {
        writeln!(&mut writer, "{}\t{}\t{}\t{}", user.rid, user.username, user.uac, pwned).expect("Failed to write to file");
    }

    writer.flush().expect("Failed to finish writing");

    let elapsed = start.elapsed();
    let minutes = elapsed.as_secs() / 60;
    let seconds = elapsed.as_secs_f32() - (minutes * 60) as f32;
    println!("Finished in {minutes} minutes {seconds:.2} seconds");
    println!("{total_users} users; {active_users} active users; {pwned_users} pwned users");
}
