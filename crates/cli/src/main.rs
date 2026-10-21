//! `zeidex` — scriptable front end over zeidex-core.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use zeidex_core::format::{common_category, common_targets};
use zeidex_core::ops::{tools_for, Op, TOOLS};
use zeidex_core::runner::{self, Event};
use zeidex_core::tools::{locate, Tool};
use zeidex_core::{integrate, Error};

const USAGE: &str = "\
zeidex — local file converter (images, video, audio, PDF, office, ebooks, archives)

USAGE:
  zeidex targets <files...>                 formats and tools available for the files
  zeidex convert --to <ext> <files...>      convert, output saved beside each original
  zeidex tool <id> [--key value]... <files...>
  zeidex info <file>                        print metadata
  zeidex tools                              list tool ids and their options
  zeidex doctor                             show which engines were found
  zeidex integrate                          add \"Open with Zeidex\" to the file manager

EXAMPLES:
  zeidex convert --to jpg *.heic
  zeidex tool compress --target 8MB clip.mov
  zeidex tool ocr --lang ara+eng --to pdf scan.pdf
  zeidex tool merge-pdf a.pdf b.pdf
";

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
        print!("{USAGE}");
        return Ok(true);
    };
    match cmd.as_str() {
        "targets" => {
            let (_, files) = split_args(rest)?;
            let cat = common_category(&files);
            println!("category: {}", cat.map_or("mixed", |c| c.name()));
            println!("convert:  {}", common_targets(&files).join(" "));
            let ids: Vec<&str> = tools_for(cat, files.len()).iter().map(|t| t.id).collect();
            println!("tools:    {}", ids.join(" "));
            Ok(true)
        }
        "convert" => {
            let (opts, files) = split_args(rest)?;
            let to = opts.get("to").ok_or_else(|| Error::BadOption("--to <ext> is required".into()))?;
            run(&Op::Convert { to: to.to_ascii_lowercase() }, &files)
        }
        "tool" => {
            let (id, rest) = rest.split_first().ok_or_else(|| Error::BadOption("tool id required".into()))?;
            let (opts, files) = split_args(rest)?;
            let op = Op::parse(id, &opts)?;
            if op == Op::Info {
                return info(&files);
            }
            run(&op, &files)
        }
        "info" => info(&split_args(rest)?.1),
        "tools" => {
            for t in TOOLS {
                let cats: Vec<&str> = t.cats.iter().map(|c| c.name()).collect();
                let params: Vec<String> = t.params.iter().map(|p| format!("--{} {:?}", p.key, p.default)).collect();
                println!("{:<14} {:<22} [{}]{} {}", t.id, t.label, cats.join(","), if t.multi { " (2+ files)" } else { "" }, params.join(" "));
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
            print!("{USAGE}");
            Ok(true)
        }
        other => Err(Error::BadOption(format!("unknown command `{other}` (try --help)"))),
    }
}

fn info(files: &[PathBuf]) -> Result<bool, Error> {
    for f in files {
        println!("== {}", f.display());
        println!("{}", runner::info(f)?);
    }
    Ok(true)
}

fn run(op: &Op, files: &[PathBuf]) -> Result<bool, Error> {
    if files.is_empty() {
        return Err(Error::BadOption("no input files".into()));
    }
    let tty = std::io::stderr().is_terminal();
    let clear = if tty { "\r\x1b[2K" } else { "" };
    let results = runner::run(op, files, |e| match e {
        Event::Start { index, total, file } => eprint!("[{}/{}] {file} ", index + 1, total),
        Event::Progress(f) if tty => {
            eprint!("\r\x1b[2K  {:>3}%", (f * 100.0) as u32);
            let _ = std::io::stderr().flush();
        }
        Event::Progress(_) => {}
        Event::Done(p) => eprintln!("{clear}  -> {}", p.display()),
        Event::Failed { error, .. } => eprintln!("{clear}  failed: {error}"),
    });
    for p in results.iter().flatten() {
        println!("{}", p.display());
    }
    Ok(results.iter().all(Result::is_ok))
}
