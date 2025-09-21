use std::{
    error::Error,
    fmt,
    fs::{File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
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

fn timestamp_filename(timestamp: u64) -> String {
    format!("schema_{timestamp}.graphql")
}

fn write_to_file<P: AsRef<Path>>(
    schema_str: &str,
    filepath: P,
    print_success: bool,
) -> Result<(), anyhow::Error> {
    let mut file = open_file(&filepath)
        .with_context(|| format!("cannot open file {}", filepath.as_ref().display()))?;
    file.write_all(schema_str.as_bytes())
        .with_context(|| format!("cannot write to file {}", filepath.as_ref().display()))?;

    if print_success {
        println!(
            "✔ Generated GraphQL schema at {}",
            filepath.as_ref().display()
        );
    }

    Ok(())
}

#[derive(Debug)]
struct AggregateError {
    errors: Vec<anyhow::Error>,
}

impl fmt::Display for AggregateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, err) in self.errors.iter().enumerate() {
            writeln!(f, "{}: {err:#}", i + 1)?;
        }
        Ok(())
    }
}

impl Error for AggregateError {}

fn join_results<I>(errors: I) -> Result<(), AggregateError>
where
    I: IntoIterator<Item = anyhow::Result<()>>,
{
    let errors = errors
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AggregateError { errors })
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let schema = graphql_api::schema::create_schema_standalone();
    let schema_str = schema.sdl();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("cannot get current timestamp")?
        .as_secs();

    let result_a = if args.directory.is_none() && args.output.is_none() && !args.stdout {
        write_to_file(
            &schema_str,
            PathBuf::from(timestamp_filename(timestamp)),
            true,
        )
    } else {
        Ok(())
    };

    let result_b = match args.directory {
        Some(dir) => write_to_file(
            &schema_str,
            Path::new(&dir).join(timestamp_filename(timestamp)),
            !args.stdout,
        ),
        None => Ok(()),
    };

    let result_c = match args.output {
        Some(filename) => write_to_file(&schema_str, filename, !args.stdout),
        None => Ok(()),
    };

    if args.stdout {
        println!("{schema_str}");
    }

    join_results([result_a, result_b, result_c]).context("couldn't generate a file")
}
