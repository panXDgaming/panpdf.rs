use convert_structure::bytes_tool::{Fonts, Input as Document, Run, Settings};

use crate::catalogue::{Tool, Values};
use crate::run::collect::Collected;
use crate::run::encode;
use crate::run::failure;
use crate::run::names::{Names, base_name, made_unique};
use crate::run::{Context, Failure, Input, Outcome, Progress, Watch};

fn settings_of(tool: Tool, values: &Values) -> Settings {
    let mut settings = Settings::default();
    for (key, text) in encode::pairs(tool, values) {
        settings.set(key, &text);
    }
    settings
}

fn fonts_of(context: &Context) -> Fonts {
    Fonts {
        provider: context.fonts.clone(),
        files: Vec::new(),
    }
}

pub fn each_document(
    run: Run,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    context: &Context,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let settings = settings_of(tool, values);
    let fonts = fonts_of(context);
    let of = inputs.len();
    let several = of > 1;
    let mut names = Names::default();
    let mut made = Collected::default();
    if context.fonts.is_none() && matches!(tool, Tool::ExcelToPdf | Tool::PowerPointToPdf) {
        made.note("no fonts were given, so the pages come out without their text");
    }
    for (index, input) in inputs.into_iter().enumerate() {
        watch.check()?;
        watch.report(Progress::File { index, of });
        watch.report(Progress::Busy);
        let name = base_name(&input.name).to_owned();
        let one = [Document {
            name: name.clone(),
            bytes: input.bytes,
        }];
        match run(&one, &settings, &fonts, &mut |_, _| !watch.stopped()) {
            Ok(done) => {
                made.notes_of(&name, done.notes, false);
                for (file, bytes) in done.files {
                    made.files.push((names.claim(&file), bytes));
                }
            }
            Err(message) => made.fail(&name, failure::from_engine(&message))?,
        }
    }
    made.finish(several)
}

pub fn all_at_once(
    run: Run,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    context: &Context,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let settings = settings_of(tool, values);
    let fonts = fonts_of(context);
    let documents: Vec<Document> = inputs
        .into_iter()
        .map(|input| Document {
            name: input.name,
            bytes: input.bytes,
        })
        .collect();
    watch.report(Progress::Busy);
    let done = run(&documents, &settings, &fonts, &mut |done, total| {
        let go = watch.step(done, total);
        if done == total {
            watch.report(Progress::Writing);
        }
        go
    })
    .map_err(|message| failure::from_engine(&message))?;
    watch.check()?;
    Ok(Outcome {
        files: made_unique(done.files),
        notes: done.notes,
    })
}
