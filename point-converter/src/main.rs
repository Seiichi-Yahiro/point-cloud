use std::fs::read_dir;
use std::path::PathBuf;

use clap::{arg, Parser};
use itertools::Itertools;

use point_converter::convert_from_paths;

/// Point converter will convert your points to a format that the point cloud renderer can use.
/// Currently supported file formats are las/laz and ply and the generated metadata.json.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None, verbatim_doc_comment)]
struct Args {
    /// Output directory of the converted format
    /// Will be created if it doesn't exist
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Directories with input files to convert
    #[arg(short, long, value_name = "DIRS")]
    directories: Vec<PathBuf>,

    /// Input files with the points to convert
    #[arg(short, long, value_name = "FILES")]
    files: Vec<PathBuf>,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    let dirs = args
        .directories
        .iter()
        .map(|path| read_dir(path).unwrap())
        .flat_map(|dir| dir.into_iter().map(|dir_entry| dir_entry.unwrap().path()));

    let files = args.files.iter().cloned().chain(dirs).collect_vec();

    if files.is_empty() {
        log::warn!("Please provide some files or directories");
        return;
    }

    convert_from_paths(
        &files,
        args.output
            .unwrap_or_else(|| std::env::current_dir().unwrap()),
    );
}
