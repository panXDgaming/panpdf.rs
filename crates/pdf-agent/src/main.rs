use std::io::{BufRead, Write};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("panpdf-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let mut server = pdf_agent::protocol::Server::default();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        match input.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("panpdf-mcp: standard input: {error}");
                break;
            }
        }
        if let Some(reply) = server.answer_bytes(&line)
            && (writeln!(output, "{reply}").is_err() || output.flush().is_err())
        {
            break;
        }
    }
}
