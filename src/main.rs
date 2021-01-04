use libc::{c_int, c_ulong, c_void, termios as Termios, winsize as WinSize};
use libc::{BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, STDIN_FILENO, STDOUT_FILENO, TIOCGWINSZ};
use std::io::{self, Error, ErrorKind, Read, Result};
use std::mem;

extern "C" {
    pub fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
    pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
    pub fn iscntrl(c: c_int) -> c_int;
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

trait TermiosAttrExt {
    fn get_attr(&mut self) -> Result<()>;
    fn set_attr(&self) -> Result<()>;
}

impl TermiosAttrExt for Termios {
    fn get_attr(&mut self) -> Result<()> {
        Ok(unsafe {
            if tcgetattr(STDIN_FILENO, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }

    fn set_attr(&self) -> Result<()> {
        Ok(unsafe {
            if tcsetattr(STDIN_FILENO, libc::TCSAFLUSH, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }
}

struct EditorConfig {
    orig_termios: Termios,
    curr_termios: Termios,
    window_size: WinSize,
}

impl EditorConfig {
    pub fn new() -> Result<Self> {
        let mut orig_flags = unsafe { mem::zeroed::<Termios>() };
        let ws = unsafe { mem::zeroed::<WinSize>() };

        orig_flags.get_attr()?;

        Ok(Self {
            orig_termios: orig_flags,
            curr_termios: orig_flags.clone(),
            window_size: ws,
        })
    }

    pub fn enable_raw_mode(&mut self) -> Result<()> {
        self.curr_termios.c_lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
        self.curr_termios.c_iflag &= !(IXON | ICRNL | BRKINT | INPCK | ISTRIP);
        self.curr_termios.c_oflag &= !(OPOST);
        self.curr_termios.c_oflag |= CS8;
        self.curr_termios.set_attr()?;
        Ok(())
    }

    pub fn get_window_size(&mut self) -> Result<()> {
        unsafe {
            if ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut self.window_size) == -1 || self.window_size.ws_col == 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get window size"));
            } else {
                Ok(())
            }
        }
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

const fn ctrl_key(c: char) -> u8 {
    c as u8 & 0x1F
}

const EXIT: u8 = ctrl_key('q');

fn write_terminal(seq: &str, len: usize) {
    unsafe {
        libc::write(STDOUT_FILENO, seq.as_ptr() as *const c_void, len);
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

fn editor_draw_rows(e: &mut EditorConfig) {
    for _ in 0..e.window_size.ws_col {
        write_terminal("~\r\n", 3);
    }
}

fn editor_refresh_screen(e: &mut EditorConfig) {
    write_terminal("\x1b[2J", 4);
    write_terminal("\x1b[H", 3);

    editor_draw_rows(e);

    write_terminal("\x1b[H", 3);
}

fn main() -> Result<()> {
    let mut run = true;
    let mut editor = EditorConfig::new()?;

    editor.enable_raw_mode()?;
    editor.get_window_size()?;

    while run {
        editor_refresh_screen(&mut editor);
        run = editor_process_keypress()?;
    }

    editor_refresh_screen(&mut editor);
    Ok(())
}
