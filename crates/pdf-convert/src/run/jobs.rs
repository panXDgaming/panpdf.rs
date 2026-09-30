use convert_pdftool::{Job, Tool as Engine};

use crate::catalogue::{Choice, Made, Setting, Tool, Values};
use crate::run::collect::Collected;
use crate::run::encode::{self, Prepared};
use crate::run::failure;
use crate::run::names::{Names, stem};
use crate::run::{Failure, Input, Outcome, Progress, Watch};

fn job_of(inputs: Vec<Vec<u8>>, prepared: Prepared) -> Job {
    Job {
        inputs,
        options: prepared
            .options
            .into_iter()
            .map(|(key, text)| (key.to_owned(), text))
            .collect(),
        files: prepared
            .files
            .into_iter()
            .map(|(key, bytes)| (key.to_owned(), bytes))
            .collect(),
        random: Vec::new(),
    }
}

pub fn each_file(
    engine: &'static Engine,
    suffix: &str,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let of = inputs.len();
    let several = of > 1;
    let mut names = Names::default();
    let mut made = Collected::default();
    for (index, input) in inputs.into_iter().enumerate() {
        watch.check()?;
        watch.report(Progress::File { index, of });
        let prepared = encode::prepare(tool, values)?;
        let notes = prepared.notes.clone();
        let job = job_of(vec![input.bytes], prepared);
        watch.report(Progress::Busy);
        let output = (engine.run)(&job);
        watch.check()?;
        match output {
            Ok(output) => {
                let wanted = format!("{}{suffix}.{}", stem(&input.name), engine.extension);
                made.files.push((names.claim(&wanted), output.main));
                made.notes_of(&input.name, [notes, output.notes].concat(), several);
            }
            Err(message) => made.fail(&input.name, failure::from_engine(&message))?,
        }
    }
    made.finish(several)
}

pub fn compare(
    engine: &'static Engine,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let prepared = encode::prepare(tool, values)?;
    let job = job_of(
        inputs.into_iter().map(|input| input.bytes).collect(),
        prepared,
    );
    watch.report(Progress::Busy);
    let output = (engine.run)(&job).map_err(|message| failure::from_engine(&message))?;
    watch.check()?;
    let extension = if values.choice(Setting::ReportAs) == Some(Choice::TextReport) {
        "txt"
    } else {
        Made::Report.extension()
    };
    Ok(Outcome {
        files: vec![(format!("comparison.{extension}"), output.main)],
        notes: output.notes,
    })
}
