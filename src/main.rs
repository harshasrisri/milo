use libc::{c_int, c_void, termios as Termios};
use libc::{BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST};
use std::io::{self, Error, ErrorKind, Read, Result};
use std::mem;

extern "C" {
    pub fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
    pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
    pub fn iscntrl(c: c_int) -> c_int;
}

trait TermiosAttrExt {
    fn get_attr(&mut self) -> Result<()>;
    fn set_attr(&self) -> Result<()>;
}

impl TermiosAttrExt for Termios {
    fn get_attr(&mut self) -> Result<()> {
        Ok(unsafe {
            if tcgetattr(libc::STDIN_FILENO, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }

    fn set_attr(&self) -> Result<()> {
        Ok(unsafe {
            if tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }
}

struct EditorConfig {
    orig_termios: Termios,
    curr_termios: Termios,
}

impl EditorConfig {
    pub fn new() -> Result<Self> {
        let mut orig_flags = unsafe { mem::zeroed::<Termios>() };
        orig_flags.get_attr()?;
        Ok(Self {
            orig_termios: orig_flags,
            curr_termios: orig_flags.clone(),
        })
    }
}

impl Drop for EditorConfig {
    fn drop(&mut self) {
        // print!("Restoring terminal\r\n");
        self.orig_termios
            .set_attr()
            .expect("Failed to restore terminal state");
    }
}

fn enable_raw_mode(terminal: &mut Termios) -> Result<()> {
    terminal.c_lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
    terminal.c_iflag &= !(IXON | ICRNL | BRKINT | INPCK | ISTRIP);
    terminal.c_oflag &= !(OPOST);
    terminal.c_oflag |= CS8;
    terminal.set_attr()?;
    Ok(())
}

const fn ctrl_key(c: char) -> u8 {
    c as u8 & 0x1F
}

const EXIT: u8 = ctrl_key('q');

fn write_terminal(seq: &str, len: usize) {
    unsafe {
        libc::write(libc::STDOUT_FILENO, seq.as_ptr() as *const c_void, len);
    }
}

fn editor_read_key() -> Result<u8> {
    io::stdin()
        .bytes()
        .next()
        .expect("Failed to read from stdin")
}

fn editor_process_keypress() -> Result<bool> {
    match editor_read_key()? {
        EXIT => return Ok(false),
        _key => return Ok(true),
    }
}

fn editor_draw_rows() {
    for _ in 0..24 {
        write_terminal("~\r\n", 3);
    }
}

fn editor_refresh_screen() {
    write_terminal("\x1b[2J", 4);
    write_terminal("\x1b[H", 3);

    editor_draw_rows();

    write_terminal("\x1b[H", 3);
}

fn main() -> Result<()> {
    let mut run = true;
    let mut editor = EditorConfig::new()?;

    enable_raw_mode(&mut editor.curr_termios)?;

    while run {
        editor_refresh_screen();
        run = editor_process_keypress()?;
    }

    editor_refresh_screen();
    Ok(())
}
