use std::io::{self, Read, Result, Error, ErrorKind};
use libc::{termios as Termios, c_int, tcflag_t};
use std::mem;

trait TermiosAttrExt {
    fn get_attr(&mut self) -> Result<()>;
    fn set_attr(&self) -> Result<()>;
}

impl TermiosAttrExt for Termios {
    fn get_attr(&mut self) -> Result<()> {
        Ok( unsafe {
            if tcgetattr(libc::STDIN_FILENO, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }

    fn set_attr(&self) -> Result<()> {
        Ok( unsafe {
            if tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, self) != 0 {
                return Err(Error::new(ErrorKind::Other, "Can't get term attributes"));
            }
        })
    }
}

enum TermioFlagFields {
    InputFlags,
    OutputFlags,
    ControlFlags,
    LocalFlags,
}

extern "C" {
    pub fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int; 
    pub fn tcsetattr(fd: c_int, optional_actions: c_int, termios: *const Termios) -> c_int;
}

struct Terminal {
    orig_flags: Termios,
    curr_flags: Termios,
}

impl Terminal {
    pub fn new() -> Result<Self> {
        let (mut orig_flags, mut curr_flags) = unsafe {(mem::zeroed::<Termios>(), mem::zeroed::<Termios>())};
        orig_flags.get_attr()?;
        curr_flags.get_attr()?;
        Ok(Self { orig_flags, curr_flags })
    }

    pub fn enable_flag(&mut self, field: TermioFlagFields, flag: tcflag_t) -> Result<()> {
        let curr_field = match field {
            TermioFlagFields::InputFlags   => &mut self.curr_flags.c_iflag,
            TermioFlagFields::OutputFlags  => &mut self.curr_flags.c_oflag,
            TermioFlagFields::ControlFlags => &mut self.curr_flags.c_cflag,
            TermioFlagFields::LocalFlags   => &mut self.curr_flags.c_lflag,
        };
        *curr_field |= flag as u32;
        self.curr_flags.set_attr()?;
        Ok(())
    }

    pub fn disable_flag(&mut self, field: TermioFlagFields, flag: tcflag_t) -> Result<()> {
        let curr_field = match field {
            TermioFlagFields::InputFlags   => &mut self.curr_flags.c_iflag,
            TermioFlagFields::OutputFlags  => &mut self.curr_flags.c_oflag,
            TermioFlagFields::ControlFlags => &mut self.curr_flags.c_cflag,
            TermioFlagFields::LocalFlags   => &mut self.curr_flags.c_lflag,
        };
        *curr_field &= !(flag as u32);
        self.curr_flags.set_attr()?;
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.orig_flags.set_attr().expect("Failed to restore terminal state");
    }
}

fn main() -> Result<()> {
    let mut terminal = Terminal::new()?;
    terminal.disable_flag(TermioFlagFields::LocalFlags, libc::ECHO)?;
    let mut buf = [0; 1];
    loop {
        let n = io::stdin().read(&mut buf)?;
        if n == 0 || buf[0] == b'q' || buf[0] == b'Q' {
            return Ok(());
        }
    }
}
