use convert_files::{Job, Settings, Start};

use crate::catalogue::{Tool, Values};
use crate::run::collect::Collected;
use crate::run::encode;
use crate::run::failure;
use crate::run::names::made_unique;
use crate::run::{Failure, Input, Outcome, Progress, Watch};

fn settings_of(tool: Tool, values: &Values) -> Settings {
    Settings(
        encode::pairs(tool, values)
            .into_iter()
            .map(|(key, text)| (key.to_owned(), text))
            .collect(),
    )
}

fn engine_inputs(tool: Tool, inputs: Vec<Input>) -> Vec<convert_files::Input> {
    inputs
        .into_iter()
        .map(|input| convert_files::Input {
            name: if tool == Tool::Ocr {
                as_a_pdf_name(&input.name)
            } else {
                input.name
            },
            bytes: input.bytes,
        })
        .collect()
}

fn as_a_pdf_name(name: &str) -> String {
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    format!("{stem}.pdf")
}

pub fn one_job(
    start: Start,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let settings = settings_of(tool, values);
    let outcome = drive(start, engine_inputs(tool, inputs), &settings, watch)?;
    Ok(Outcome {
        files: made_unique(outcome.files),
        notes: outcome.notes,
    })
}

pub fn each_file(
    start: Start,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let settings = settings_of(tool, values);
    let of = inputs.len();
    let several = of > 1;
    let mut made = Collected::default();
    for (index, input) in inputs.into_iter().enumerate() {
        watch.check()?;
        watch.report(Progress::File { index, of });
        let name = input.name.clone();
        match drive(start, engine_inputs(tool, vec![input]), &settings, watch) {
            Ok(outcome) => {
                made.notes_of(&name, outcome.notes, several);
                made.files.extend(outcome.files);
            }
            Err(failure) => made.fail(&name, failure)?,
        }
    }
    made.files = made_unique(std::mem::take(&mut made.files));
    made.finish(several)
}

fn drive(
    start: Start,
    inputs: Vec<convert_files::Input>,
    settings: &Settings,
    watch: &mut Watch,
) -> Result<convert_files::Outcome, Failure> {
    let mut job: Box<dyn Job> =
        start(inputs, settings).map_err(|message| failure::from_engine(&message))?;
    let total = job.total();
    if total == 0 {
        watch.report(Progress::Busy);
    } else {
        watch.report(Progress::Step { done: 0, total });
    }
    for done in 1..=total {
        watch.check()?;
        job.step()
            .map_err(|message| failure::from_engine(&message))?;
        watch.report(Progress::Step { done, total });
    }
    watch.check()?;
    watch.report(Progress::Writing);
    job.finish()
        .map_err(|message| failure::from_engine(&message))
}
