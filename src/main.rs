use libc::{c_int, c_ulong, c_void, termios as Termios, winsize as WinSize};
use libc::{
    BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, STDIN_FILENO,
    STDOUT_FILENO, TIOCGWINSZ, VMIN, VTIME,
};
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
        unsafe {
            if tcgetattr(STDIN_FILENO, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        }
        Ok(())
    }

    fn set_attr(&self) -> Result<()> {
        unsafe {
            if tcsetattr(STDIN_FILENO, libc::TCSAFLUSH, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        }
        Ok(())
    }
}

struct EditorConfig {
    orig_termios: Termios,
    curr_termios: Termios,
    window_size: WinSize,
    term_buffer: String,
    cursor_col: usize,
    cursor_row: usize,
}

impl EditorConfig {
    pub fn new() -> Result<Self> {
        let mut orig_flags = unsafe { mem::zeroed::<Termios>() };
        let ws = unsafe { mem::zeroed::<WinSize>() };

        orig_flags.get_attr()?;

        Ok(Self {
            orig_termios: orig_flags,
            curr_termios: orig_flags,
            window_size: ws,
            term_buffer: String::new(),
            cursor_col: 0,
            cursor_row: 0,
        })
    }

    pub fn enable_raw_mode(&mut self) -> Result<()> {
        self.curr_termios.c_lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
        self.curr_termios.c_iflag &= !(IXON | ICRNL | BRKINT | INPCK | ISTRIP);
        self.curr_termios.c_oflag &= !(OPOST);
        self.curr_termios.c_oflag |= CS8;
        self.curr_termios.c_cc[VMIN] = 0;
        self.curr_termios.c_cc[VTIME] = 1;
        self.curr_termios.set_attr()?;
        Ok(())
    }

    pub fn get_window_size(&mut self) -> Result<()> {
        unsafe {
            if ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut self.window_size) == -1
                || self.window_size.ws_col == 0
            {
                let botright = "\x1b[999C\x1b[999B";
                if write_terminal(botright) != botright.len() as i32 {
                    return Err(Error::new(ErrorKind::Other, "Can't get window size"));
                }
                self.get_cursor_position()
            } else {
                Ok(())
            }
        }
    }

    fn get_cursor_position(&mut self) -> Result<()> {
        write_terminal("\x1b[6n");
        print!("\r\n");

        let mut cursor_buf = Vec::new();
        while let Ok(key) = editor_read_key() {
            match key {
                Key::AlphaNum(b'R') => break,
                Key::AlphaNum(c) => cursor_buf.push(c),
                _ => panic!("Unexpected input read"),
            }
        }

        let dimensions = cursor_buf[2..]
            .split(|&c| c == b';')
            .filter_map(|buf| std::str::from_utf8(buf).ok())
            .filter_map(|buf| buf.parse().ok())
            .collect::<Vec<u16>>();

        if dimensions.len() != 2 {
            return Err(Error::new(ErrorKind::Other, "Can't get window size"));
        }

        self.window_size.ws_row = dimensions[0];
        self.window_size.ws_col = dimensions[1];

        Ok(())
    }

    fn append(&mut self, content: &str) {
        self.term_buffer.push_str(content);
    }

    fn flush(&mut self) {
        write_terminal(self.term_buffer.as_str());
        self.term_buffer.clear();
    }
}

impl Drop for EditorConfig {
    fn drop(&mut self) {
        // print!("Restoring terminal\r\n");
        write_terminal("\x1b[2J");
        write_terminal("\x1b[H");
        self.orig_termios
            .set_attr()
            .expect("Failed to restore terminal state");
    }
}

fn write_terminal(seq: &str) -> c_int {
    unsafe { libc::write(STDOUT_FILENO, seq.as_ptr() as *const c_void, seq.len()) as c_int }
}

#[derive(Debug)]
enum Motion {
    Up,
    Down,
    Left,
    Right,
    PgUp,
    PgDn,
}

#[derive(Debug)]
enum Key {
    AlphaNum(u8),
    Move(Motion),
    Control(char),
}

fn editor_read_key() -> Result<Key> {
    let read_key = || io::stdin().bytes().next();
    let key = std::iter::repeat_with(read_key)
        .skip_while(|c| c.is_none())
        .flatten()
        .next()
        .unwrap()?;

    Ok(if key == b'\x1b' {
        let seq0 = if let Some(res) = read_key() {
            res?
        } else {
            return Ok(Key::AlphaNum(key));
        };

        if seq0 == b'[' {
            let seq1 = if let Some(res) = read_key() {
                res?
            } else {
                return Ok(Key::AlphaNum(key));
            };

            match seq1 {
                b'A' => Key::Move(Motion::Up),
                b'B' => Key::Move(Motion::Down),
                b'C' => Key::Move(Motion::Right),
                b'D' => Key::Move(Motion::Left),
                b'0'..=b'9' => {
                    let seq2 = if let Some(res) = read_key() {
                        res?
                    } else {
                        return Ok(Key::AlphaNum(key));
                    };
                    if seq2 == b'~' {
                        match seq1 {
                            b'5' => Key::Move(Motion::PgUp),
                            b'6' => Key::Move(Motion::PgDn),
                            _ => panic!(
                                "Unknown Escape Sequence Encountered - \\x1b[{}{}",
                                seq1, seq2
                            ),
                        }
                    } else {
                        panic!(
                            "Unknown Escape Sequence Encountered - \\x1b[{}{}",
                            seq1, seq2
                        );
                    }
                }
                _ => panic!("Unknown Escape Sequence Encountered - \\x1b[{}", seq1),
            }
        } else {
            Key::AlphaNum(key)
        }
    } else if key < 32 {
        Key::Control((key + 64) as char)
    } else {
        Key::AlphaNum(key)
    })
}

fn editor_move_cursor(e: &mut EditorConfig, motion: Motion) {
    match motion {
        Motion::Up => e.cursor_row = e.cursor_row.saturating_sub(1),
        Motion::Left => e.cursor_col = e.cursor_col.saturating_sub(1),
        Motion::Down => {
            e.cursor_row = std::cmp::min(e.window_size.ws_row as usize - 1, e.cursor_row + 1)
        }
        Motion::Right => {
            e.cursor_col = std::cmp::min(e.window_size.ws_col as usize - 1, e.cursor_col + 1)
        }
        Motion::PgUp => e.cursor_row = e.cursor_row.saturating_sub(e.window_size.ws_row as usize),
        Motion::PgDn => {
            e.cursor_row = std::cmp::min(
                e.window_size.ws_row as usize - 1,
                e.cursor_col + e.window_size.ws_row as usize,
            )
        }
    }
}

fn editor_process_keypress(e: &mut EditorConfig) -> Result<bool> {
    let key = editor_read_key()?;
    match key {
        Key::Control('Q') => Ok(false),
        Key::Move(motion) => {
            editor_move_cursor(e, motion);
            Ok(true)
        }
        _key => Ok(true),
    }
}

fn editor_draw_rows(e: &mut EditorConfig) {
    let mut banner = format!(
        "{} -- version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    banner.truncate(e.window_size.ws_col as usize);
    let padding = (e.window_size.ws_col as usize - banner.len()) / 2;
    let banner = if padding == 0 {
        banner
    } else {
        let mut centered = "~".to_string();
        centered.extend(std::iter::repeat(" ").take(padding - 1));
        centered.push_str(banner.as_str());
        centered
    };

    e.append(
        std::iter::repeat("~")
            .take(e.window_size.ws_row as usize)
            .enumerate()
            .map(|(n, buf)| {
                if n == e.window_size.ws_row as usize / 3 {
                    banner.clone()
                } else {
                    buf.to_string()
                }
            })
            .map(|mut buf| {
                buf.push_str("\x1b[K");
                buf
            })
            .collect::<Vec<_>>()
            .join("\r\n")
            .as_str(),
    );
}

fn editor_refresh_screen(e: &mut EditorConfig) {
    e.append("\x1b[?25l");
    // e.append("\x1b[2J");
    e.append("\x1b[H");

    editor_draw_rows(e);

    e.append(format!("\x1b[{};{}H", e.cursor_row + 1, e.cursor_col + 1).as_str());
    e.append("\x1b[?25h");
    e.flush();
}

fn main() -> Result<()> {
    let mut run = true;
    let mut editor = EditorConfig::new()?;

    editor.enable_raw_mode()?;
    editor.get_window_size()?;

    while run {
        editor_refresh_screen(&mut editor);
        run = editor_process_keypress(&mut editor)?;
    }

    Ok(())
}
