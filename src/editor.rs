use crate::buffer::Buffer;
use crate::terminal::{Key, Motion, Terminal};
use std::fs::File;
use std::io::Result;
use std::io::{BufRead, BufReader};
use std::time::Instant;

const STATUS_HEIGHT: usize = 2; // 1 for Status bar. 1 for Status Message
const TOTAL_QUIT_COUNT: usize = 4;
const FILE_NAME_WIDTH: usize = 20;
const STATUS_LINE_BLANK: char = ' ';

enum SearchDirection {
    Forward,
    Reverse,
}

pub struct Editor {
    terminal: Terminal,
    buffer: Buffer,
    status_msg: String,
    status_msg_ts: Instant,
    quit_count: usize,
}

impl Editor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            terminal: Terminal::new()?,
            buffer: Buffer::new(),
            status_msg: String::new(),
            status_msg_ts: Instant::now(),
            quit_count: TOTAL_QUIT_COUNT,
        })
    }

    pub fn rows(&self) -> usize {
        self.terminal.rows() - STATUS_HEIGHT
    }

    pub fn cols(&self) -> usize {
        self.terminal.cols()
    }

    pub fn keep_alive(&self) -> bool {
        self.quit_count > 0
    }

    pub fn set_status(&mut self, msg: String) {
        self.status_msg = msg;
        self.status_msg_ts = Instant::now();
    }

    pub fn process_keypress(&mut self) -> Result<()> {
        let key = self.terminal.read_key()?;

        match key {
            Key::Control('Q') => {
                if self.buffer.is_dirty() && self.quit_count > 0 {
                    self.quit_count -= 1;
                    self.set_status(format!(
                        "WARNING!!! Press Ctrl-Q {} more times to quit. File has unsaved changes.",
                        self.quit_count
                    ));
                } else {
                    self.quit_count = 0;
                }
                return Ok(()); // To prevent resetting QUIT_COUNT
            }
            Key::Control('S') => self.save()?,
            Key::Control('F') => self.find(SearchDirection::Forward),
            Key::Control('G') => self.find(SearchDirection::Reverse),
            Key::Move(motion) => self.buffer.move_cursor(motion, self.rows(), self.cols()),
            Key::Printable(ch) => self.buffer.insert_char(ch),
            Key::Tab => self.buffer.insert_char('\t'),
            Key::Newline => self.buffer.insert_new_line(),
            Key::Escape | Key::Control('L') => {}
            Key::Backspace | Key::Control('H') => self.buffer.delete_char(),
            Key::Delete => {
                self.buffer
                    .move_cursor(Motion::Right, self.rows(), self.cols());
                self.buffer.delete_char();
            }
            _key => {}
        };
        self.quit_count = TOTAL_QUIT_COUNT;
        Ok(())
    }

    pub fn open(&mut self, file_arg: Option<String>) -> Result<()> {
        if let Some(file) = file_arg {
            self.buffer.set_filename(Some(file.clone()));
            let line_iter = BufReader::new(File::open(file)?).lines();
            for line in line_iter {
                self.buffer.append_row(line?);
            }
        }
        self.buffer.not_dirty();
        Ok(())
    }

    fn save(&mut self) -> Result<()> {
        if self.buffer.filename().is_none() {
            let some_name = self.prompt("Save as (ESC to cancel): ");
            self.buffer.set_filename(some_name);
        }
        if let Some(filename) = &self.buffer.filename() {
            let content = self.buffer.rows_to_string();
            if let Err(err) = std::fs::write(filename, content.as_bytes()) {
                self.set_status(format!("Can't save! I/O error: {}", err));
                return Err(err);
            }
            self.set_status(format!("{} bytes written to disk", content.len()));
            self.buffer.not_dirty();
        } else {
            self.set_status("Filename not set!!!".to_string());
        }
        Ok(())
    }

    fn find(&mut self, direction: SearchDirection) {
        let mut query = String::new();
        let cursor = self.buffer.cursor_position();
        loop {
            let (finished, pending_key) =
                self.prompt_incremental("Search (Use ESC/Arrows/Enter): ", &mut query);
            if finished {
                break;
            }
            let (row, col) = match pending_key {
                Some(Key::Move(Motion::Up)) | Some(Key::Move(Motion::Left)) => {
                    self.buffer.find_reverse(&query, true)
                }
                Some(Key::Move(Motion::Down)) | Some(Key::Move(Motion::Right)) => {
                    self.buffer.find_forward(&query, true)
                }
                _ => match direction {
                    SearchDirection::Forward => self.buffer.find_forward(&query, false),
                    SearchDirection::Reverse => self.buffer.find_reverse(&query, false),
                },
            };

            self.buffer.place_cursor(row, col);
        }
        if query.is_empty() {
            self.buffer.set_cursor_position(cursor);
        }
    }

    fn draw_content(&self) -> String {
        if self.buffer.is_empty() {
            crate::editor_home_screen(self.rows(), self.cols())
        } else {
            self.buffer.frame_content(self.rows(), self.cols())
        }
    }

    fn draw_status_bar(&self) -> String {
        let filename = self
            .buffer
            .filename()
            .as_ref()
            .map(|file| file.to_str().unwrap_or("<file-name-not-utf8>"))
            .unwrap_or("[No Name]");
        let status_left = format!(
            "{name:<.*} - {lc} lines {dirty}",
            FILE_NAME_WIDTH,
            name = filename,
            lc = self.buffer.line_count(),
            dirty = if self.buffer.is_dirty() {
                "(modified)"
            } else {
                ""
            },
        );
        let c_row = self.buffer.cursor_position().cursor_row;
        let status_right = format!("{}/{}", c_row + 1, self.buffer.line_count());
        let num_spaces = self
            .cols()
            .saturating_sub(status_left.len())
            .saturating_sub(status_right.len());

        format!(
            "\x1b[7m{left}{:spaces$}{right}\x1b[m\r\n",
            STATUS_LINE_BLANK,
            spaces = num_spaces,
            left = status_left,
            right = status_right
        )
    }

    fn draw_message_bar(&mut self) {
        self.terminal.append("\x1b[K");
        if self.status_msg_ts.elapsed().as_secs() < 5 {
            self.status_msg.truncate(self.cols());
            let msg = self.status_msg.clone();
            self.terminal.append(msg.as_str());
        }
    }

    pub fn refresh_screen(&mut self) {
        self.terminal.refresh().unwrap_or(());
        self.buffer.scroll(self.rows(), self.cols());

        self.terminal.append("\x1b[?25l");
        self.terminal.append("\x1b[H");

        self.terminal.append(&self.draw_content());
        self.terminal.append(&self.draw_status_bar());
        self.draw_message_bar();

        let (c_row, c_col) = self.buffer.cursor_placement();
        self.terminal
            .append(format!("\x1b[{};{}H", c_row, c_col).as_str());
        self.terminal.append("\x1b[?25h");
        self.terminal.flush();
    }

    fn prompt_incremental(
        &mut self,
        prompt: &str,
        incremental: &mut String,
    ) -> (bool, Option<Key>) {
        self.set_status(format!("{}{}", prompt, incremental));
        self.refresh_screen();
        match self.terminal.read_key().unwrap_or(Key::Escape) {
            Key::Printable(ch) => {
                incremental.push(ch);
                (false, None)
            }
            Key::Newline => {
                self.set_status(String::new());
                (true, None)
            }
            Key::Escape => {
                incremental.clear();
                self.set_status(String::new());
                (true, None)
            }
            Key::Delete | Key::Backspace | Key::Control('H') => {
                incremental.pop();
                (false, None)
            }
            key => (false, Some(key)),
        }
    }

    fn prompt(&mut self, prompt: &str) -> Option<String> {
        let mut reply = String::new();
        loop {
            if self.prompt_incremental(prompt, &mut reply).0 {
                return if reply.is_empty() { None } else { Some(reply) };
            }
        }
    }
}
