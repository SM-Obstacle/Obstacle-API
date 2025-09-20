use std::{
    fs::{File, OpenOptions},
    io::{self, Write as _},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use clap::{Parser as _, ValueHint};

#[derive(clap::Parser)]
struct Args {
    #[arg(short, long, value_hint = ValueHint::DirPath)]
    directory: Option<String>,
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    output: Option<String>,
    #[arg(long, default_value_t = false)]
    stdout: bool,
}

fn open_file<P: AsRef<Path>>(path: P) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let schema = graphql_api::schema::create_schema_standalone();
    let schema_str = schema.sdl();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("cannot get current timestamp")?
        .as_secs();

    if args.directory.is_none() && args.output.is_none() && !args.stdout {
        let filename = format!("schema_{timestamp}.graphql");
        let mut file =
            open_file(&filename).with_context(|| format!("cannot open file {filename}"))?;
        file.write_all(schema_str.as_bytes())
            .with_context(|| format!("cannot write to file {filename}"))?;
        if !args.stdout {
            println!("✔ Generated GraphQL schema at {filename}");
        }
    }

    if let Some(dir) = args.directory {
        let filename = Path::new(&dir).join(format!("schema_{timestamp}.graphql"));
        let mut file = open_file(&filename)
            .with_context(|| format!("cannot open file {}", filename.display()))?;
        file.write_all(schema_str.as_bytes())
            .with_context(|| format!("cannot write to file {}", filename.display()))?;
        if !args.stdout {
            println!("✔ Generated GraphQL schema at {}", filename.display());
        }
    }

    if let Some(filename) = args.output {
        let mut file =
            open_file(&filename).with_context(|| format!("cannot open file {filename}"))?;
        file.write_all(schema_str.as_bytes())
            .with_context(|| format!("cannot write to file {filename}"))?;
        if !args.stdout {
            println!("✔ Generated GraphQL schema at {filename}");
        }
    }

    if args.stdout {
        println!("{schema_str}");
    }

    Ok(())
}
