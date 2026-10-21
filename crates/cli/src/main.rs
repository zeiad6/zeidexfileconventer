//! `zeidex` — scriptable front end over zeidex-core.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use zeidex_core::format::{common_category, common_targets};
use zeidex_core::ops::{parse_step, tools_for_paths, Op, TOOLS};
use zeidex_core::runner::{self, Event};
use zeidex_core::tools::{locate, Tool};
use zeidex_core::{integrate, Error};

const USAGE: &str = r#"
zeidex — local file converter (images, video, audio, PDF, office, ebooks, archives)

USAGE:
  zeidex targets <files...>                 formats and tools available for the files
  zeidex convert --to <ext> <files...>      convert, output saved beside each original
  zeidex tool <id> [--key value]... <files...>
  zeidex recipe "<step> | <step> ..." <files...>   chain tools, e.g. "resize max=1600 | compress percent=60 | strip"
  zeidex watch <dir> --do "<step or recipe>" [--ext heic,png]   process files as they arrive (Ctrl+C stops)
  zeidex info <file>                        print metadata
  zeidex qr-make "<text>" [--out qr.png]    make a QR code image (needs qrencode)
  zeidex tools                              list tool ids and their options
  zeidex doctor                             show which engines were found
  zeidex integrate                          add "Open with Zeidex" to the file manager

EXAMPLES:
  zeidex convert --to jpg *.heic
  zeidex tool compress --target 8MB clip.mov
  zeidex tool ocr --lang ara+eng --to pdf scan.pdf
  zeidex tool merge-pdf a.pdf b.pdf
  zeidex tool rename --pattern "trip-{n:3}" *.jpg
  zeidex watch ~/Downloads --ext heic --do "convert to=jpg"

OPTIONS:
  --jobs N    files processed at the same time (default: half the CPU cores, max 4)
"#;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match real_main(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

/// Splits `--key value` pairs from positional file arguments.
fn split_args(args: &[String]) -> Result<(HashMap<String, String>, Vec<PathBuf>), Error> {
    let (mut opts, mut files) = (HashMap::new(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--" {
            files.extend(it.by_ref().map(PathBuf::from));
        } else if let Some(key) = a.strip_prefix("--") {
            let val = it.next().ok_or_else(|| Error::BadOption(format!("--{key} needs a value")))?;
            opts.insert(key.to_string(), val.clone());
        } else {
            files.push(PathBuf::from(a));
        }
    }
    Ok((opts, files))
}

fn real_main(args: &[String]) -> Result<bool, Error> {
    let Some((cmd, rest)) = args.split_first() else {
        print!("{}", USAGE.trim_start());
        return Ok(true);
    };
    match cmd.as_str() {
        "targets" => {
            let (_, files) = split_args(rest)?;
            let cat = common_category(&files);
            println!("category: {}", cat.map_or("mixed", |c| c.name()));
            println!("convert:  {}", common_targets(&files).join(" "));
            let ids: Vec<&str> = tools_for_paths(&files).iter().map(|t| t.id).collect();
            println!("tools:    {}", ids.join(" "));
            Ok(true)
        }
        "convert" => {
            let (opts, files) = split_args(rest)?;
            let to = opts.get("to").ok_or_else(|| Error::BadOption("--to <ext> is required".into()))?;
            run(&Op::Convert { to: to.to_ascii_lowercase() }, &files, &opts)
        }
        "recipe" => {
            let (spec, rest) = rest.split_first().ok_or_else(|| Error::BadOption("recipe steps required".into()))?;
            let (opts, files) = split_args(rest)?;
            run(&Op::Recipe(zeidex_core::ops::parse_recipe(spec)?), &files, &opts)
        }
        "watch" => {
            let (opts, dirs) = split_args(rest)?;
            let dir = dirs.first().ok_or_else(|| Error::BadOption("folder required".into()))?;
            let op = parse_step(opts.get("do").ok_or_else(|| Error::BadOption("--do \"<step>\" is required".into()))?)?;
            let exts: Vec<String> =
                opts.get("ext").map_or(vec![], |e| e.split(',').map(|x| x.trim().to_ascii_lowercase()).collect());
            watch(dir, &exts, &op)
        }
        "tool" => {
            let (id, rest) = rest.split_first().ok_or_else(|| Error::BadOption("tool id required".into()))?;
            let (opts, files) = split_args(rest)?;
            let op = Op::parse(id, &opts)?;
            if runner::is_text_tool(&op) {
                print!("{}", runner::text_tool(&op, &files)?);
                return Ok(true);
            }
            run(&op, &files, &opts)
        }
        "info" => {
            print!("{}", runner::text_tool(&Op::Info, &split_args(rest)?.1)?);
            Ok(true)
        }
        "qr-make" => {
            let (text, rest) = rest.split_first().ok_or_else(|| Error::BadOption("text required".into()))?;
            let (opts, _) = split_args(rest)?;
            let out = PathBuf::from(opts.get("out").map_or("qr.png", String::as_str));
            let out = runner::unique_path(&std::path::absolute(&out)?);
            let ok = std::process::Command::new(zeidex_core::tools::require(Tool::Qrencode)?)
                .args(["-s", "10", "-m", "2", "-l", "M", "-o"])
                .arg(&out)
                .arg("--")
                .arg(text)
                .status()?
                .success();
            if ok {
                println!("{}", out.display());
            }
            Ok(ok)
        }
        "tools" => {
            for t in TOOLS {
                let cats: Vec<&str> = t.cats.iter().map(|c| c.name()).collect();
                let params: Vec<String> = t.params.iter().map(|p| format!("--{} {:?}", p.key, p.default)).collect();
                println!(
                    "{:<14} {:<22} [{}]{} {}",
                    t.id,
                    t.label,
                    cats.join(","),
                    if t.multi { " (2+ files)" } else { "" },
                    params.join(" ")
                );
            }
            Ok(true)
        }
        "doctor" => {
            let mut all = true;
            for t in Tool::ALL {
                match locate(t) {
                    Some(p) => println!("  ok       {:<14} {}", t.label(), p.display()),
                    None => {
                        all = false;
                        println!("  missing  {:<14} {}", t.label(), t.install_hint());
                    }
                }
            }
            Ok(all)
        }
        "integrate" => {
            let exe = std::env::current_exe()?;
            for p in integrate::install(&exe)? {
                println!("wrote {}", p.display());
            }
            Ok(true)
        }
        "-h" | "--help" | "help" => {
            print!("{}", USAGE.trim_start());
            Ok(true)
        }
        other => Err(Error::BadOption(format!("unknown command `{other}` (try --help)"))),
    }
}

/// Polls `dir` once a second and runs `op` on every new, fully written file.
fn watch(dir: &std::path::Path, exts: &[String], op: &Op) -> Result<bool, Error> {
    if !dir.is_dir() {
        return Err(Error::BadOption(format!("{} is not a folder", dir.display())));
    }
    let mut w = zeidex_core::watch::Watcher::new(dir, exts);
    eprintln!("watching {} (Ctrl+C to stop)", dir.display());
    loop {
        for file in w.poll() {
            for r in runner::run(op, std::slice::from_ref(&file), |_| {}) {
                match r {
                    Ok(out) => {
                        w.mark_done(&out);
                        println!("{} -> {}", file.display(), out.display());
                    }
                    Err(e) => eprintln!("{}: {e}", file.display()),
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn run(op: &Op, files: &[PathBuf], opts: &HashMap<String, String>) -> Result<bool, Error> {
    if files.is_empty() {
        return Err(Error::BadOption("no input files".into()));
    }
    let jobs = match opts.get("jobs") {
        Some(j) => j.parse::<usize>().map_err(|_| Error::BadOption(format!("jobs={j}")))?.max(1),
        None => runner::default_parallelism(),
    };
    let tty = std::io::stderr().is_terminal();
    let clear = if tty { "\r\x1b[2K" } else { "" };
    // With parallel units, per-file progress lines would interleave: show it only for one file.
    let live = tty && (jobs == 1 || files.len() == 1 || op.is_multi());
    let results = runner::run_with(op, files, jobs, |e| match e {
        Event::Start { index, total, file } if live => eprint!("[{}/{}] {file} ", index + 1, total),
        Event::Start { .. } => {}
        Event::Progress { fraction, .. } if live => {
            eprint!("\r\x1b[2K  {:>3}%", (fraction * 100.0) as u32);
            let _ = std::io::stderr().flush();
        }
        Event::Progress { .. } => {}
        Event::Done { output, .. } => eprintln!("{clear}  -> {}", output.display()),
        Event::Failed { file, error, .. } => eprintln!("{clear}  {file}: {error}"),
    });
    for p in results.iter().flatten() {
        println!("{}", p.display());
    }
    Ok(results.iter().all(Result::is_ok))
}
