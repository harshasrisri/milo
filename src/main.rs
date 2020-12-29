use std::io::{self, Read};

fn main() -> io::Result<()> {
    let mut stdin = io::stdin();
    let mut buf = [0; 1];
    loop {
        let n = stdin.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
    }
}
