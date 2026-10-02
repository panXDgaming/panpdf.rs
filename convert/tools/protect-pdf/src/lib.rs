use convert_pdfdoc::{Load, Protection};
use convert_pdftool::{Job, Output, Tool};
use pdf_security::{Allowed, PrintAllowance, Wanted};

pub static TOOL: Tool = Tool {
    name: "protect-pdf",
    extension: "pdf",
    inputs: 1,
    file_options: &[],
    flags: &[],
    help: "  --password PW          the password that opens the file (may be empty with --owner-password)\n  \
           --owner-password PW    the password that allows everything (default: the same as --password)\n  \
           --deny LIST            what a reader may not do: print,print-high,copy,modify,annotate,forms,assemble\n  \
           --input-password PW    when the file is already protected",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

pub fn allowed(deny: &str) -> Result<Allowed, String> {
    let mut allowed = Allowed::default();
    for word in deny
        .split([',', ' '])
        .map(str::trim)
        .filter(|w| !w.is_empty())
    {
        match word {
            "print" => allowed.print = PrintAllowance::Refused,
            "print-high" => {
                if allowed.print == PrintAllowance::Faithful {
                    allowed.print = PrintAllowance::Degraded;
                }
            }
            "copy" => allowed.copy = false,
            "modify" => allowed.modify = false,
            "annotate" => allowed.annotate = false,
            "forms" => allowed.fill_forms = false,
            "assemble" => allowed.assemble = false,
            other => {
                return Err(format!(
                    "--deny: '{other}' is not one of print, print-high, copy, modify, annotate, forms, assemble"
                ));
            }
        }
    }
    Ok(allowed)
}

pub fn run(job: &Job) -> Result<Output, String> {
    let user = job.get("password").unwrap_or_default().to_owned();
    let owner = job
        .get("owner-password")
        .map_or_else(|| user.clone(), ToOwned::to_owned);
    if user.is_empty() && owner.is_empty() {
        return Err("give a password (--password), or an owner password (--owner-password)".into());
    }
    let allowed = allowed(job.get("deny").unwrap_or_default())?;
    if !job.random.is_empty() {
        pdf_security::seed_random(&job.random)
            .map_err(|e| format!("the random bytes handed over were refused ({e})"))?;
    }
    let mut notes = Vec::new();
    if allowed != Allowed::default() && owner == user {
        notes.push(
            "the permissions bind only a reader who does not know the owner password; \
             with no --owner-password the opening password is also the owner's, so anyone \
             who can open the file can lift them"
                .to_owned(),
        );
    }
    let bytes = job.pdf()?.to_vec();
    let compress = convert_pdfdoc::uses_object_streams(&bytes);
    let doc = convert_pdfdoc::load(
        bytes,
        &Load {
            password: job
                .get("input-password")
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| e.to_string())?;
    if doc.recovered {
        notes.push(format!(
            "the file's cross-reference was damaged and was rebuilt to read it ({} repairs)",
            doc.notes.len()
        ));
    }
    if convert_pdfdoc::is_signed(&doc) {
        notes.push("the file's digital signatures do not survive protecting it".into());
    }
    let pages = doc.pages().len();
    let made = pdf_security::make_protection(&Wanted {
        user: user.as_bytes().to_vec(),
        owner: owner.as_bytes().to_vec(),
        allowed,
    })
    .map_err(|e| {
        format!("the protection could not be made ({e}); in a browser, hand over 64 bytes from crypto.getRandomValues through random() before run()")
    })?;
    let written = convert_pdfdoc::write(
        &doc,
        Some(&Protection {
            dictionary: &made.dictionary,
            security: &made.security,
        }),
        compress,
    )?;
    convert_pdfdoc::check_written(&written.bytes, user.as_bytes(), pages)?;
    convert_pdfdoc::check_written(&written.bytes, owner.as_bytes(), pages)?;
    notes.push(format!(
        "protected with AES-256 (PDF 2.0), {pages} pages{}",
        if user.is_empty() {
            "; it opens without a password and the permissions apply".to_owned()
        } else {
            String::new()
        }
    ));
    Ok(Output {
        main: written.bytes,
        attachments: Vec::new(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use convert_pdftool::Job;

    #[test]
    fn unfilled_random_bytes_are_refused_before_anything_is_written() {
        let mut job = Job {
            random: vec![0; 64],
            ..Job::default()
        };
        job.options.insert("password".into(), "a".into());
        let refused = super::run(&job).unwrap_err();
        assert!(
            refused.starts_with("the random bytes handed over were refused"),
            "{refused}"
        );
        job.random = (0..=63).collect();
        let next = super::run(&job).unwrap_err();
        assert_eq!(next, "no input file was given");
    }
}
