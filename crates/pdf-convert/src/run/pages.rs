use std::sync::Arc;

use convert_structure::{FontProvider, Output, Request};

use crate::catalogue::{Setting, Tool, Values};
use crate::run::collect::Collected;
use crate::run::encode;
use crate::run::failure;
use crate::run::names::{Names, stem};
use crate::run::{Failure, Input, Outcome, Progress, Watch};

pub type Convert = fn(
    Vec<u8>,
    &Request,
    Option<Arc<dyn FontProvider>>,
    &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String>;

pub fn convert_all(
    convert: Convert,
    tool: Tool,
    inputs: Vec<Input>,
    values: &Values,
    fonts: Option<&Arc<dyn FontProvider>>,
    watch: &mut Watch,
) -> Result<Outcome, Failure> {
    let pages = encode::requested_pages(values)?;
    let password = encode::secret(values, Setting::Password);
    let extension = tool.output().extension();
    let of = inputs.len();
    let several = of > 1;
    let mut names = Names::default();
    let mut made = Collected::default();
    for (index, input) in inputs.into_iter().enumerate() {
        watch.check()?;
        watch.report(Progress::File { index, of });
        let main = names.free(&format!("{}.{extension}", stem(&input.name)));
        let folder = format!("{}_files", stem(&main));
        let request = Request {
            pages: pages.clone(),
            password: password.clone(),
            attachments: folder.clone(),
        };
        let converted = convert(input.bytes, &request, fonts.cloned(), &mut |done, total| {
            let go = watch.step(done, total);
            if done == total {
                watch.report(Progress::Writing);
            }
            go
        });
        match converted {
            Ok(output) => {
                names.keep(&main);
                made.notes_of(&input.name, output.notes, several);
                if let Some(note) = left_out(&output.dropped) {
                    made.notes_of(&input.name, vec![note], several);
                }
                made.files.push((main, output.main));
                for (name, bytes) in output.attachments {
                    made.files.push((format!("{folder}/{name}"), bytes));
                }
            }
            Err(message) => made.fail(&input.name, failure::from_engine(&message))?,
        }
    }
    made.finish(several)
}

fn left_out(dropped: &str) -> Option<String> {
    let squeezed: Vec<&str> = dropped.split_whitespace().collect();
    if squeezed.is_empty() {
        return None;
    }
    let text = squeezed.join(" ");
    let shown: String = text.chars().take(80).collect();
    let more = if shown.len() < text.len() { "..." } else { "" };
    Some(format!(
        "running heads and unplaced text left out: {shown}{more}"
    ))
}
