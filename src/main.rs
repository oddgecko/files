use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn print_usage(prog: &str) {
    println!("Usage:");
    println!("  {prog}                  interactive (list + ask partition + mount)");
    println!("  {prog} sdb3             mount /dev/sdb3");
    println!("  {prog} /dev/sda6        mount /dev/sda6");
    println!("  {prog} mount sdb3       mount /dev/sdb3");
    println!("  {prog} unmount sdb3     unmount /dev/sdb3  (also: umount, u)");
    println!("  {prog} list             show partitions (also: ls, status)");
}

fn show_partitions() {
    // Prefer lsblk for a compact, familiar table.
    let out = Command::new("lsblk")
        .args(["-o", "NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
        }
        _ => {
            // Fallback to udisksctl status.
            let _ = Command::new("udisksctl")
                .arg("status")
                .stdin(Stdio::inherit())
                .status();
        }
    }
}

/// Normalize user input like `sdb3`, `/dev/sdb3`, `sda6` into `/dev/...`.
/// Returns None on empty / unsafe input.
fn normalize_partition(input: &str) -> Option<String> {
    let t = input.trim().trim_matches('"').trim_matches('\'').trim();
    if t.is_empty() {
        return None;
    }
    let dev = if t.starts_with("/dev/") {
        t.to_string()
    } else {
        // Allow bare names like `sdb3`. Strip a stray leading `/`.
        let bare = t.trim_start_matches('/');
        format!("/dev/{bare}")
    };
    // Safety: only allow /dev/<alnum + - _>. No spaces, shell metachars, or `..`.
    let rest = dev.strip_prefix("/dev/")?;
    if rest.is_empty()
        || rest.contains("..")
        || rest.contains(' ')
        || !rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' )
    {
        return None;
    }
    // Reject nvme-style `p` partitions? No — allow digits too, already covered.
    // Must look like a block device.
    if !Path::new(&dev).exists() {
        eprintln!("{dev} does not exist. Check `list` for valid names.");
        return None;
    }
    Some(dev)
}

fn prompt(text: &str) -> String {
    print!("{text}");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

/// Run `udisksctl mount -b <dev>`, inheriting stdin so polkit can
/// ask for your system password if needed. Returns mountpoint on success.
fn do_mount(dev: &str) -> Result<String, i32> {
    println!("Running: udisksctl mount -b {dev}");
    println!("(enter your login password if polkit asks — that prompt comes from the system, not this tool)");
    let status = Command::new("udisksctl")
        .args(["mount", "-b", dev])
        .stdin(Stdio::inherit())
        .status()
        .map_err(|e| {
            eprintln!("failed to run udisksctl: {e}");
            1
        })?;
    if !status.success() {
        return Err(status.code().unwrap_or(1));
    }
    // Query mountpoint so we can show / open it.
    let mp = Command::new("lsblk")
        .args(["-no", "MOUNTPOINT", dev])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    if mp.is_empty() {
        println!("Mounted {dev} (mountpoint unknown, run `lsblk` to see it).");
    } else {
        println!("Mounted {dev} at {mp}");
    }
    Ok(mp)
}

fn do_unmount(dev: &str) -> Result<(), i32> {
    println!("Running: udisksctl unmount -b {dev}");
    let status = Command::new("udisksctl")
        .args(["unmount", "-b", dev])
        .stdin(Stdio::inherit())
        .status()
        .map_err(|e| {
            eprintln!("failed to run udisksctl: {e}");
            1
        })?;
    if !status.success() {
        return Err(status.code().unwrap_or(1));
    }
    println!("Unmounted {dev}");
    Ok(())
}

fn maybe_open(mp: &str) {
    if mp.is_empty() {
        return;
    }
    let ans = prompt(format!("Open {mp} in file manager? [Y/n]: ").as_str());
    if ans.is_empty() || ans.eq_ignore_ascii_case("y") || ans.eq_ignore_ascii_case("yes") {
        let _ = Command::new("xdg-open").arg(mp).spawn();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("files");

    // --- CLI modes ---
    if args.len() >= 2 {
        let a = args[1].to_lowercase();
        if a == "-h" || a == "--help" || a == "help" {
            print_usage(prog);
            return;
        }
        if a == "list" || a == "ls" || a == "status" {
            show_partitions();
            return;
        }
        if (a == "mount" || a == "m") && args.len() >= 3 {
            match normalize_partition(&args[2]) {
                Some(dev) => match do_mount(&dev) {
                    Ok(mp) => maybe_open(&mp),
                    Err(c) => std::process::exit(c),
                },
                None => {
                    eprintln!("Invalid partition: {}", args[2]);
                    std::process::exit(2);
                }
            }
            return;
        }
        if (a == "unmount" || a == "umount" || a == "u") && args.len() >= 3 {
            match normalize_partition(&args[2]) {
                Some(dev) => {
                    if do_unmount(&dev).is_err() {
                        std::process::exit(1);
                    }
                }
                None => {
                    eprintln!("Invalid partition: {}", args[2]);
                    std::process::exit(2);
                }
            }
            return;
        }
        // Bare partition: `files sdb3`
        if args.len() == 2 {
            match normalize_partition(&args[1]) {
                Some(dev) => match do_mount(&dev) {
                    Ok(mp) => maybe_open(&mp),
                    Err(c) => std::process::exit(c),
                },
                None => {
                    eprintln!("Invalid partition: {}", args[1]);
                    print_usage(prog);
                    std::process::exit(2);
                }
            }
            return;
        }
        print_usage(prog);
        std::process::exit(2);
    }

    // --- Interactive mode ---
    println!("Available disks/partitions:");
    show_partitions();
    println!();
    println!("Examples: sdb3, sda6, sda4  (your other boots)");
    let action = prompt("Action: [m]ount (default) / [u]nmmount / [l]ist? ");
    let a = action.to_lowercase();
    if a == "l" || a == "list" || a == "ls" {
        // Already shown; just exit.
        return;
    }
    let unmount = a == "u" || a == "unmount" || a == "umount";

    let part = prompt(if unmount {
        "Partition to unmount: "
    } else {
        "Partition to mount: "
    });
    let Some(dev) = normalize_partition(&part) else {
        eprintln!("Invalid partition `{part}`. Use e.g. sdb3 or /dev/sdb3.");
        std::process::exit(2);
    };
    if unmount {
        if do_unmount(&dev).is_err() {
            std::process::exit(1);
        }
    } else {
        match do_mount(&dev) {
            Ok(mp) => maybe_open(&mp),
            Err(c) => std::process::exit(c),
        }
    }
}
