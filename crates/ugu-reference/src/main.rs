// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::path::PathBuf;
use std::process::ExitCode;

use ugu_reference::{Error, compare, reference, viewer};

const USAGE: &str =
    "usage: ugu-reference compare <a> <b> --matrix <matrix.json> --out <directory> [--labels A,B]

Compares two reference exports of every scene in the matrix, writes
report.json and a side-by-side viewer (index.html) into the output directory,
and prints the per-group summary.";

struct Options {
    a: PathBuf,
    b: PathBuf,
    matrix: PathBuf,
    output: PathBuf,
    labels: [String; 2],
}

fn parse(arguments: &[String]) -> Option<Options> {
    let [command, a, b, rest @ ..] = arguments else {
        return None;
    };
    if command != "compare" {
        return None;
    }
    let (mut matrix, mut output) = (None, None);
    let mut labels = ["A".to_owned(), "B".to_owned()];
    for pair in rest.chunks(2) {
        match pair {
            [flag, value] if flag == "--matrix" => matrix = Some(PathBuf::from(value)),
            [flag, value] if flag == "--out" => output = Some(PathBuf::from(value)),
            [flag, value] if flag == "--labels" => {
                let (first, second) = value.split_once(',')?;
                labels = [first.to_owned(), second.to_owned()];
            }
            _ => return None,
        }
    }
    Some(Options {
        a: PathBuf::from(a),
        b: PathBuf::from(b),
        matrix: matrix?,
        output: output?,
        labels,
    })
}

fn run(options: &Options) -> Result<(), Error> {
    let matrix = reference::read_matrix(&options.matrix)?;
    let report = compare::compare(&matrix, &options.a, &options.b)?;
    viewer::write(
        &report,
        &options.a,
        &options.b,
        [&options.labels[0], &options.labels[1]],
        &options.output,
    )?;
    for (group, metrics) in &report.summary {
        println!("{group}");
        for (metric, statistics) in metrics {
            println!(
                "  {metric:<34} median {:>10.4}  p95 {:>10.4}  max {:>10.4}",
                statistics.median, statistics.p95, statistics.max
            );
        }
    }
    println!("{}", options.output.join("index.html").display());
    Ok(())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(options) = parse(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
