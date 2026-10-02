use std::path::{Path, PathBuf};

#[must_use]
pub fn named_for_pages(original: &Path, pages: &[usize]) -> String {
    let stem = original
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let tail = match pages {
        [] => String::new(),
        [one] => format!("-p{}", one + 1),
        [first, .., last] if last - first + 1 == pages.len() => {
            format!("-p{}-{}", first + 1, last + 1)
        }
        many => format!("-{}pages", many.len()),
    };
    format!("{stem}{tail}.pdf")
}

#[must_use]
pub fn files_written(base: &Path, spread: usize) -> Vec<PathBuf> {
    files_written_as(base, spread, "pdf")
}

#[must_use]
pub fn files_written_as(base: &Path, spread: usize, ending: &str) -> Vec<PathBuf> {
    if spread <= 1 {
        return vec![base.to_path_buf()];
    }
    (1..=spread)
        .map(|at| numbered_as(base, at, spread, ending))
        .collect()
}

#[must_use]
pub fn numbered(base: &Path, which: usize, of: usize) -> PathBuf {
    numbered_as(base, which, of, "pdf")
}

#[must_use]
pub fn numbered_as(base: &Path, which: usize, of: usize, ending: &str) -> PathBuf {
    let digits = of.to_string().len();
    let stem = base
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let mut file = base.to_path_buf();
    file.set_file_name(format!("{stem}-{which:0digits$}.{ending}"));
    file
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{files_written, named_for_pages, numbered};

    #[test]
    fn a_file_of_pages_is_named_after_them() {
        let book = Path::new("/tmp/book.pdf");
        assert_eq!(named_for_pages(book, &[6]), "book-p7.pdf");
        assert_eq!(named_for_pages(book, &[6, 7, 8]), "book-p7-9.pdf");
        assert_eq!(named_for_pages(book, &[0, 2, 4]), "book-3pages.pdf");
        assert_eq!(named_for_pages(book, &[0, 1]), "book-p1-2.pdf");
        assert_eq!(named_for_pages(book, &[]), "book.pdf");
    }

    #[test]
    fn split_files_are_numbered_and_padded() {
        let base = Path::new("/tmp/book.pdf");
        assert_eq!(numbered(base, 1, 3), PathBuf::from("/tmp/book-1.pdf"));
        assert_eq!(numbered(base, 2, 12), PathBuf::from("/tmp/book-02.pdf"));
        assert_eq!(numbered(base, 10, 12), PathBuf::from("/tmp/book-10.pdf"));
        assert_eq!(
            numbered(Path::new("/tmp/a.b.pdf"), 1, 1),
            PathBuf::from("/tmp/a.b-1.pdf")
        );
    }

    #[test]
    fn a_save_says_which_files_it_would_write() {
        let base = Path::new("/tmp/book.pdf");
        assert_eq!(files_written(base, 1), vec![PathBuf::from("/tmp/book.pdf")]);
        assert_eq!(files_written(base, 0), vec![PathBuf::from("/tmp/book.pdf")]);
        assert_eq!(
            files_written(base, 3),
            vec![
                PathBuf::from("/tmp/book-1.pdf"),
                PathBuf::from("/tmp/book-2.pdf"),
                PathBuf::from("/tmp/book-3.pdf"),
            ]
        );
        assert_eq!(files_written(base, 11).len(), 11);
        assert_eq!(
            files_written(base, 11)[0],
            PathBuf::from("/tmp/book-01.pdf")
        );
    }
}
