use core::format_args;
use libc::{c_int, c_ulong, c_void, termios as Termios, winsize as WinSize};
use libc::{
    BRKINT, CS8, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, OPOST, STDIN_FILENO,
    STDOUT_FILENO, TIOCGWINSZ, VMIN, VTIME,
};
use std::cmp::min;
use std::fmt::Write;
use std::fs::File;
use std::io::{self, Error, ErrorKind, Read, Result};
use std::io::{BufRead, BufReader};
use std::mem;
use std::path::PathBuf;
use std::time::Instant;

extern "C" {
    pub fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
    pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
    pub fn iscntrl(c: c_int) -> c_int;
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

const TAB_STOP: usize = 8;
const STATUS_HEIGHT: usize = 2; // 1 for Status bar. 1 for Status Message
const TOTAL_QUIT_COUNT: usize = 3;

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

macro_rules! editor_set_status_message {
    ($e:expr, $($arg:tt)*) => {{
        $e.status_msg.clear();
        if let Ok(_) = $e.status_msg.write_fmt($crate::format_args!($($arg)*)) {
            $e.status_msg_ts = Instant::now();
        }
    }}
}

struct EditorState {
    orig_termios: Termios,
    curr_termios: Termios,
    window_size: WinSize,
    key_buffer: Vec<u8>,
    term_buffer: String,
    render_col: usize,
    cursor_col: usize,
    cursor_row: usize,
    keep_alive: bool,
    text_lines: Vec<String>,
    render_lines: Vec<String>,
    row_offset: usize,
    col_offset: usize,
    filename: Option<PathBuf>,
    dirty: bool,
    status_msg: String,
    status_msg_ts: Instant,
    quit_count: usize,
}

impl EditorState {
    pub fn new() -> Result<Self> {
        let mut orig_flags = unsafe { mem::zeroed::<Termios>() };
        let ws = unsafe { mem::zeroed::<WinSize>() };

        orig_flags.get_attr()?;

        Ok(Self {
            orig_termios: orig_flags,
            curr_termios: orig_flags,
            window_size: ws,
            key_buffer: Vec::new(),
            term_buffer: String::new(),
            render_col: 0,
            cursor_col: 0,
            cursor_row: 0,
            keep_alive: true,
            text_lines: Vec::new(),
            render_lines: Vec::new(),
            row_offset: 0,
            col_offset: 0,
            dirty: false,
            filename: None,
            status_msg: String::new(),
            status_msg_ts: Instant::now(),
            quit_count: TOTAL_QUIT_COUNT,
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
                self.get_cursor_position()?;
            }
            self.window_size.ws_row -= STATUS_HEIGHT as u16;
            Ok(())
        }
    }

    fn get_cursor_position(&mut self) -> Result<()> {
        write_terminal("\x1b[6n");
        print!("\r\n");

        let mut cursor_buf = Vec::new();
        while let Ok(key) = editor_read_key(self) {
            match key {
                Key::Printable(b'R') => break,
                Key::Printable(c) => cursor_buf.push(c),
                _ => {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "Unexpected input while reading window size",
                    ))
                }
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

impl Drop for EditorState {
    fn drop(&mut self) {
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
    Home,
    End,
}

#[derive(Debug)]
enum Key {
    Printable(u8),
    Move(Motion),
    Control(char),
    Delete,
    Backspace,
    Newline,
    Escape,
}

fn editor_read_key(e: &mut EditorState) -> Result<Key> {
    let read_key = || io::stdin().bytes().next();
    let key = if let Some(pending_key) = e.key_buffer.pop() {
        pending_key
    } else {
        std::iter::repeat_with(read_key)
            .skip_while(|c| c.is_none())
            .flatten()
            .next()
            .unwrap()?
    };

    Ok(if key == b'\x1b' {
        let seq = e
            .key_buffer
            .iter()
            .rev()
            .map(|byte| Some(Ok(*byte)))
            .chain(std::iter::repeat_with(read_key))
            .take(3)
            .map(|k| k.transpose())
            .collect::<Result<Vec<Option<u8>>>>()?;

        let (key, pending) = match seq.as_slice() {
            [None, None, None] => (Key::Escape, None),

            [Some(b'['), Some(b'A'), pending] => (Key::Move(Motion::Up), *pending),
            [Some(b'['), Some(b'B'), pending] => (Key::Move(Motion::Down), *pending),
            [Some(b'['), Some(b'C'), pending] => (Key::Move(Motion::Right), *pending),
            [Some(b'['), Some(b'D'), pending] => (Key::Move(Motion::Left), *pending),

            [Some(b'['), Some(b'5'), Some(b'~')] => (Key::Move(Motion::PgUp), None),
            [Some(b'['), Some(b'6'), Some(b'~')] => (Key::Move(Motion::PgDn), None),

            [Some(b'['), Some(b'1'), Some(b'~')] => (Key::Move(Motion::Home), None),
            [Some(b'['), Some(b'7'), Some(b'~')] => (Key::Move(Motion::Home), None),
            [Some(b'['), Some(b'O'), Some(b'H')] => (Key::Move(Motion::Home), None),
            [Some(b'['), Some(b'H'), pending] => (Key::Move(Motion::Home), *pending),

            [Some(b'['), Some(b'4'), Some(b'~')] => (Key::Move(Motion::End), None),
            [Some(b'['), Some(b'8'), Some(b'~')] => (Key::Move(Motion::End), None),
            [Some(b'['), Some(b'O'), Some(b'F')] => (Key::Move(Motion::End), None),
            [Some(b'['), Some(b'F'), pending] => (Key::Move(Motion::End), *pending),

            [Some(b'['), Some(b'3'), Some(b'~')] => (Key::Delete, None),

            _ => {
                e.key_buffer.clear();
                e.key_buffer.extend(seq.iter().rev().filter_map(|&k| k));
                (editor_read_key(e)?, None)
            }
        };

        if let Some(key) = pending {
            e.key_buffer.push(key);
        }

        key
    } else {
        match key {
            127 => Key::Backspace,
            b'\r' => Key::Newline,
            key if key < 32 => Key::Control((key + 64) as char),
            key => Key::Printable(key),
        }
    })
}

fn editor_move_cursor(e: &mut EditorState, motion: Motion) {
    match motion {
        Motion::Up => e.cursor_row = e.cursor_row.saturating_sub(1),
        Motion::Left => {
            if e.cursor_col != 0 {
                e.cursor_col -= 1;
            } else if e.cursor_row > 0 {
                e.cursor_row -= 1;
                e.cursor_col = e.text_lines[e.cursor_row].len();
            }
        }
        Motion::Down => e.cursor_row = min(e.text_lines.len().saturating_sub(1), e.cursor_row + 1),
        Motion::Right => {
            if let Some(row) = e.text_lines.get(e.cursor_row) {
                if e.cursor_col < row.len() {
                    e.cursor_col += 1;
                } else if e.cursor_row < e.text_lines.len() - 1 {
                    e.cursor_row += 1;
                    e.cursor_col = 0;
                }
            }
        }
        Motion::PgUp => e.cursor_row = e.cursor_row.saturating_sub(e.window_size.ws_row as usize),
        Motion::PgDn => {
            e.cursor_row = min(
                e.text_lines.len().saturating_sub(1),
                e.cursor_row + e.window_size.ws_row as usize,
            )
        }
        Motion::Home => e.cursor_col = 0,
        Motion::End => e.cursor_col = e.window_size.ws_col as usize - 1,
    }

    if let Some(row) = e.text_lines.get(e.cursor_row) {
        e.cursor_col = min(row.len(), e.cursor_col);
    }
}

fn editor_process_keypress(e: &mut EditorState) -> Result<()> {
    let key = editor_read_key(e)?;

    match key {
        Key::Control('Q') => {
            if e.dirty && e.quit_count > 0 {
                editor_set_status_message!(
                    e,
                    "WARNING!!! File has unsaved changes. Press Ctrl-Q {} more times to quit.",
                    e.quit_count
                );
                e.quit_count -= 1;
            } else {
                e.keep_alive = false;
            }
            return Ok(()); // To prevent resetting QUIT_COUNT
        }
        Key::Control('S') => editor_save(e)?,
        Key::Move(motion) => editor_move_cursor(e, motion),
        Key::Printable(ch) => editor_insert_char(e, ch as char),
        Key::Newline
        | Key::Escape
        | Key::Control('L')
        | Key::Backspace
        | Key::Delete
        | Key::Control('H') => {}
        _key => {}
    };
    e.quit_count = TOTAL_QUIT_COUNT;
    Ok(())
}

fn editor_draw_home_screen(e: &mut EditorState) {
    let mut banner = format!(
        "{} -- version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    banner.truncate(e.window_size.ws_col as usize);
    let padding = (e.window_size.ws_col as usize).saturating_sub(banner.len()) / 2;
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
                    banner.chars()
                } else {
                    buf.chars()
                }
            })
            .flat_map(|buf| buf.chain("\x1b[K\r\n".chars()))
            .collect::<String>()
            .as_str(),
    );
}

fn editor_draw_content(e: &mut EditorState) {
    e.append(
        e.render_lines
            .iter()
            .skip(e.row_offset)
            .map(|line| line.as_str())
            .chain(
                std::iter::repeat("~")
                    .take((e.window_size.ws_row as usize).saturating_sub(e.text_lines.len())),
            )
            .map(|line| {
                line.chars()
                    .skip(e.col_offset)
                    .take(e.window_size.ws_col as usize)
                    .chain("\x1b[K\r\n".chars())
            })
            .take(e.window_size.ws_row as usize)
            .flatten()
            .collect::<String>()
            .as_str(),
    );
}

fn editor_draw_rows(e: &mut EditorState) {
    if e.text_lines.is_empty() {
        editor_draw_home_screen(e)
    } else {
        editor_draw_content(e)
    }
}

const FILE_NAME_WIDTH: usize = 20;
const STATUS_LINE_BLANK: char = ' ';
fn editor_draw_status_bar(e: &mut EditorState) {
    e.append("\x1b[7m");
    let filename = e
        .filename
        .as_ref()
        .map(|file| file.to_str().unwrap_or("<file-name-not-utf8>"))
        .unwrap_or("[No Name]");
    let status_left = format!(
        "{name:<.*} - {lc} lines {dirty}",
        FILE_NAME_WIDTH,
        name = filename,
        lc = e.text_lines.len(),
        dirty = if e.dirty { "(modified)" } else { "" },
    );
    let status_right = format!("{}/{}", e.cursor_row + 1, e.text_lines.len());
    let num_spaces = (e.window_size.ws_col as usize)
        .saturating_sub(status_left.len())
        .saturating_sub(status_right.len());
    let mut status = format!(
        "{left}{:spaces$}{right}",
        STATUS_LINE_BLANK,
        spaces = num_spaces,
        left = status_left,
        right = status_right
    );
    status.truncate(e.window_size.ws_col as usize);
    e.append(status.as_str());
    e.append("\x1b[m");
    e.append("\r\n");
}

fn editor_scroll(e: &mut EditorState) {
    e.render_col = editor_row_cursor_to_render(e);

    if e.cursor_row < e.row_offset {
        e.row_offset = e.cursor_row;
    } else if e.cursor_row >= e.row_offset + e.window_size.ws_row as usize {
        e.row_offset = 1 + e.cursor_row - e.window_size.ws_row as usize;
    }

    if e.render_col < e.col_offset {
        e.col_offset = e.render_col;
    } else if e.render_col >= e.col_offset + e.window_size.ws_col as usize {
        e.col_offset = 1 + e.render_col - e.window_size.ws_col as usize;
    }
}

fn editor_draw_message_bar(e: &mut EditorState) {
    e.append("\x1b[K");
    if e.status_msg_ts.elapsed().as_secs() < 5 {
        e.status_msg.truncate(e.window_size.ws_col as usize);
        let msg = e.status_msg.clone();
        e.append(msg.as_str());
    }
}

fn editor_refresh_screen(e: &mut EditorState) {
    editor_scroll(e);

    e.append("\x1b[?25l");
    e.append("\x1b[H");

    editor_draw_rows(e);
    editor_draw_status_bar(e);
    editor_draw_message_bar(e);

    e.append(
        format!(
            "\x1b[{};{}H",
            e.cursor_row - e.row_offset + 1,
            e.render_col - e.col_offset + 1
        )
        .as_str(),
    );
    e.append("\x1b[?25h");
    e.flush();
}

fn editor_row_cursor_to_render(e: &EditorState) -> usize {
    if let Some(row) = e.text_lines.get(e.cursor_row) {
        row.chars().take(e.cursor_col).fold(0, |rx, c| {
            if c == '\t' {
                rx + (TAB_STOP - 1) - (rx % TAB_STOP)
            } else {
                rx + 1
            }
        })
    } else {
        0
    }
}

fn editor_update_row(e: &mut EditorState, row: usize) {
    match e.render_lines.len().cmp(&row) {
        std::cmp::Ordering::Equal => e.render_lines.push(String::new()),
        std::cmp::Ordering::Less => panic!("Render row index our of bounds"),
        std::cmp::Ordering::Greater => {}
    }

    let text_line = e.text_lines.get(row).expect("Text row index out of bounds");
    let render_line = &mut e.render_lines[row];
    render_line.clear();

    render_line.extend(text_line.chars().enumerate().map(|(n, c)| {
        if c == '\t' {
            std::iter::repeat(' ')
                .take(TAB_STOP - (n % TAB_STOP))
                .collect()
        } else {
            c.to_string()
        }
    }));
}

fn editor_append_row(e: &mut EditorState, line: String) {
    e.text_lines.push(line);
    e.dirty = true;
    editor_update_row(e, e.text_lines.len() - 1);
}

fn editor_row_insert_char(e: &mut EditorState, ch: char) {
    let text_line = e
        .text_lines
        .get_mut(e.cursor_row)
        .expect("Text row index out of bounds");
    e.cursor_col = min(e.cursor_col, text_line.len());
    text_line.insert(e.cursor_col, ch);
    editor_update_row(e, e.cursor_row);
    e.dirty = true;
}

fn editor_insert_char(e: &mut EditorState, ch: char) {
    if e.cursor_row == e.text_lines.len() {
        editor_append_row(e, "".to_string());
    }
    editor_row_insert_char(e, ch);
    e.cursor_col += 1;
}

fn editor_rows_to_string(e: &EditorState) -> String {
    let mut content = e.text_lines.join("\n");
    content.push('\n');
    content
}

fn editor_save(e: &mut EditorState) -> Result<()> {
    if let Some(filename) = &e.filename {
        let content = editor_rows_to_string(e);
        if let Err(err) = std::fs::write(filename, content.as_bytes()) {
            editor_set_status_message!(e, "Can't save! I/O error: {}", err);
            return Err(err);
        }
        editor_set_status_message!(e, "{} bytes written to disk", content.len());
        e.dirty = false;
    } else {
        editor_set_status_message!(e, "Filename not set!!!");
    }
    Ok(())
}

fn editor_open(e: &mut EditorState, file_arg: Option<String>) -> Result<()> {
    if let Some(file) = file_arg {
        e.filename = Some(file.clone().into());
        let line_iter = BufReader::new(File::open(file)?).lines();
        for line in line_iter {
            editor_append_row(e, line?);
        }
    }
    e.dirty = false;
    Ok(())
}

fn main() -> Result<()> {
    let mut editor = EditorState::new()?;

    editor.enable_raw_mode()?;
    editor.get_window_size()?;

    editor_open(&mut editor, std::env::args().nth(1))?;
    editor_set_status_message!(&mut editor, "HELP: Ctrl-S = save | Ctrl-Q = quit");

    while editor.keep_alive {
        editor_refresh_screen(&mut editor);
        editor_process_keypress(&mut editor)?;
    }

    Ok(())
}
