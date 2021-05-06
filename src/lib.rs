pub mod buffer;
pub mod editor;
pub mod line;
pub mod terminal;

pub fn editor_home_screen(rows: usize, cols: usize) -> String {
    let mut banner = format!(
        "{} -- version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    banner.truncate(cols);
    let padding = cols.saturating_sub(banner.len()) / 2;
    let banner = if padding == 0 {
        banner
    } else {
        let mut centered = "~".to_string();
        centered.extend(std::iter::repeat(" ").take(padding - 1));
        centered.push_str(banner.as_str());
        centered
    };

    std::iter::repeat("~")
        .take(rows)
        .enumerate()
        .map(|(n, buf)| {
            if n == rows / 3 {
                banner.chars()
            } else {
                buf.chars()
            }
        })
        .flat_map(|buf| buf.chain("\x1b[K\r\n".chars()))
        .collect()
}
