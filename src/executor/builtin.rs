extern crate libc;
use crate::shell::Shell;

pub fn run_builtin(argv: &[String], shell: &mut Shell) -> Option<i32> {
    match argv[0].as_str() {
        "cd" => Some(builtin_cd(argv, shell)),
        "exit" => std::process::exit(argv.get(1).and_then(|s| s.parse().ok()).unwrap_or(0)),
        "export" => Some(builtin_export(argv, shell)),
        "unset" => Some(builtin_unset(argv, shell)),
        "pwd" => Some(builtin_pwd(shell)),
        "fg" => Some(builtin_fg(argv, shell)),
        "bg" => Some(builtin_bg(argv, shell)),
        _ => None,
    }
}

fn builtin_cd(argv: &[String], shell: &mut Shell) -> i32 {
    let target = argv
        .get(1)
        .map(String::as_str)
        .unwrap_or_else(|| shell.get("HOME").unwrap_or("/"));

    let path = if target.starts_with('/') {
        std::path::PathBuf::from(target)
    } else {
        shell.cwd.join(target)
    };

    match std::env::set_current_dir(&path) {
        Ok(_) => {
            shell.cwd = path.canonicalize().unwrap_or(path);
            0
        }
        Err(e) => {
            eprintln!("cd: {}: {}", target, e);
            1
        }
    }
}

fn builtin_export(argv: &[String], shell: &mut Shell) -> i32 {
    for arg in &argv[1..] {
        if let Some((k, v)) = arg.split_once('=') {
            shell.set_exported(k, v);
        } else {
            shell.export(arg.as_str());
        }
    }
    0
}

fn builtin_unset(argv: &[String], shell: &mut Shell) -> i32 {
    for arg in &argv[1..] {
        shell.unset(arg);
    }
    0
}

fn builtin_pwd(shell: &Shell) -> i32 {
    println!("{}", shell.cwd.display());
    0
}

fn parse_job_arg(arg: Option<&String>, shell: &Shell) -> Option<u32> {
    let job = match arg {
        Some(s) => s.strip_prefix('%').unwrap_or(s).parse::<u32>().ok()?,
        None => shell.last_job()?,
    };
    Some(job)
}

fn builtin_fg(argv: &[String], shell: &mut Shell) -> i32 {
    let Some(job) = parse_job_arg(argv.get(1), shell) else {
        eprintln!("fg: no current job");
        return 1;
    };
    let Some(pid) = shell.job_pid(job) else {
        eprintln!("fg: %{job}: no such job");
        return 1;
    };
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
    let Some(child) = shell.remove_job(job) else {
        return 1;
    };
    let code = super::wait_foreground(shell, child);
    shell.status = code;
    code
}

fn builtin_bg(argv: &[String], shell: &mut Shell) -> i32 {
    let Some(job) = parse_job_arg(argv.get(1), shell) else {
        eprintln!("bg: no current job");
        return 1;
    };
    let Some(pid) = shell.job_pid(job) else {
        eprintln!("bg: %{job}: no such job");
        return 1;
    };
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
    println!("[{job}] running  pid={pid}");
    0
}
