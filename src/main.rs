use std::io::{self, Read, Result, Error, ErrorKind};
use libc::{termios as Termios, c_int};
use std::mem;

fn disable_echo() -> Result<()> {
    extern "C" {
        pub fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int; 
        pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
    }
    unsafe {
        let mut termio: Termios = mem::zeroed();
        if tcgetattr(libc::STDIN_FILENO, &mut termio) != 0 {
            return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
        }
        termio.c_lflag &= !(libc::ECHO);
        if tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &termio) != 0 {
            return Err(Error::new(ErrorKind::Other, "Can't set term attributes"));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    disable_echo()?;
    let mut buf = [0; 1];
    loop {
        let n = io::stdin().read(&mut buf)?;
        if n == 0 || buf[0] == b'q' || buf[0] == b'Q' {
            return Ok(());
        }
    }
}
